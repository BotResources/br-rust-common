use uuid::Uuid;

use br_core_integration::{OutboxStatus, Transition, next_after_attempt};

use crate::coords::IntegrationSubject;
use crate::fabric::Fabric;
use crate::outbox::duplicate_counter::record_duplicate;
use crate::outbox::health::{RelayHealthChannel, RelayHealthReceiver};
use crate::outbox::msg_id::{MessageIdSource, message_id_for};
use crate::outbox::report::{
    FailureClass, RelayPass, RelayPolicy, RelayReport, classify_failure, classify_pass,
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
        Ok(self.run_once_detailed().await?.into())
    }

    pub async fn run_once_detailed(&self) -> Result<RelayPass, OutboxStoreError> {
        let mut pass = RelayPass::default();
        let cap = self.policy.max_messages.max(1);
        let mut cursor = Uuid::nil();

        for _ in 0..cap {
            match self.process_one(cursor, &mut pass).await? {
                Some(id) => cursor = id,
                None => break,
            }
        }

        Ok(pass)
    }

    async fn process_one(
        &self,
        after: Uuid,
        pass: &mut RelayPass,
    ) -> Result<Option<Uuid>, OutboxStoreError> {
        let mut tx = self.pool.begin().await?;
        let Some(record) = self.store.fetch_one_pending(&mut *tx, after).await? else {
            tx.commit().await?;
            return Ok(None);
        };

        let (message_id, id_source) = message_id_for(record.id, &record.payload);
        let publish_result = self
            .fabric
            .publish_event_value_with_id(
                &record.destination,
                &record.payload,
                &message_id.to_string(),
            )
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

        pass.picked += 1;
        if id_source == MessageIdSource::Row {
            pass.row_id_fallbacks += 1;
        }
        classify_pass(pass, &publish_result, transition, structural);
        match &publish_result {
            Ok(outcome) if outcome.is_duplicate() => {
                record_duplicate();
                tracing::warn!(
                    outbox_id = %record.id,
                    message_id = %message_id,
                    id_source = id_source.as_str(),
                    sequence = outcome.sequence(),
                    subject = %record.destination.subject(),
                    "duplicate publish ack",
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(
                    outbox_id = %record.id,
                    subject = %record.destination.subject(),
                    attempts = transition.attempts,
                    structural,
                    error = %err,
                    "outbox publish attempt failed",
                );
            }
        }
        Ok(Some(record.id))
    }
}
