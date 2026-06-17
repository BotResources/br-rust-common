use uuid::Uuid;

use br_core_integration::{OutboxStatus, Transition, next_after_attempt};

use crate::fabric::Fabric;
use crate::outbox::health::{RelayHealthChannel, RelayHealthReceiver};
use crate::outbox::report::{
    FailureClass, RelayPolicy, RelayReport, classify_failure, classify_pass,
};
use crate::outbox::store::{OutboxStore, OutboxStoreError};

pub struct OutboxRelay {
    pub(super) pool: sqlx::PgPool,
    pub(super) store: OutboxStore,
    fabric: Fabric,
    pub(super) policy: RelayPolicy,
    pub(super) health: RelayHealthChannel,
}

impl OutboxRelay {
    pub fn new(pool: sqlx::PgPool, fabric: Fabric) -> Self {
        Self::with(pool, fabric, RelayPolicy::default())
    }

    pub fn with(pool: sqlx::PgPool, fabric: Fabric, policy: RelayPolicy) -> Self {
        Self {
            pool,
            store: OutboxStore::new(),
            fabric,
            policy,
            health: RelayHealthChannel::new(),
        }
    }

    pub fn health(&self) -> RelayHealthReceiver {
        self.health.receiver()
    }

    pub async fn run_once(&self) -> Result<RelayReport, OutboxStoreError> {
        let mut report = RelayReport::default();
        let cap = self.policy.max_messages.max(1);
        let mut cursor = Uuid::nil();

        for _ in 0..cap {
            match self.process_one(cursor, &mut report).await? {
                Some(id) => cursor = id,
                None => break,
            }
        }

        Ok(report)
    }

    async fn process_one(
        &self,
        after: Uuid,
        report: &mut RelayReport,
    ) -> Result<Option<Uuid>, OutboxStoreError> {
        let mut tx = self.pool.begin().await?;
        let Some(record) = self.store.fetch_one_pending(&mut *tx, after).await? else {
            tx.commit().await?;
            return Ok(None);
        };

        let publish_result = self
            .fabric
            .publish_event_value(&record.destination, &record.payload)
            .await;

        let structural =
            publish_result.as_ref().err().map(classify_failure) == Some(FailureClass::Structural);

        let transition = if structural {
            Transition {
                status: OutboxStatus::Pending,
                attempts: record.attempts,
            }
        } else {
            next_after_attempt(
                record.attempts,
                self.policy.max_attempts,
                publish_result.is_ok(),
            )
        };
        let last_error = publish_result.as_ref().err().map(|e| e.to_string());

        self.store
            .apply_transition(&mut *tx, record.id, transition, last_error.as_deref())
            .await?;
        tx.commit().await?;

        report.picked += 1;
        classify_pass(report, &publish_result, transition, structural);
        if let Err(err) = publish_result {
            tracing::warn!(
                outbox_id = %record.id,
                subject = %record.destination.subject(),
                attempts = transition.attempts,
                structural,
                error = %err,
                "outbox publish attempt failed",
            );
        }
        Ok(Some(record.id))
    }
}
