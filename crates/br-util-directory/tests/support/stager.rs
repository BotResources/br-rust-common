use std::sync::{Arc, Mutex};

use br_util_directory::{ForeignRef, Impact, ImpactStager};
use uuid::Uuid;

pub struct RecordingStager {
    seen: Arc<Mutex<Vec<Impact>>>,
    fail: bool,
}

impl RecordingStager {
    pub fn accepting() -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        }
    }

    pub fn failing() -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            fail: true,
        }
    }

    pub fn seen(&self) -> Arc<Mutex<Vec<Impact>>> {
        Arc::clone(&self.seen)
    }
}

#[async_trait::async_trait]
impl ImpactStager for RecordingStager {
    async fn stage_in(
        &self,
        conn: &mut sqlx::PgConnection,
        impacts: &[Impact],
    ) -> Result<(), br_util_directory::DirectoryError> {
        for impact in impacts {
            let foreign = foreign_ref(impact);
            sqlx::query("INSERT INTO staged_impact (namespace, key) VALUES ($1, $2)")
                .bind(foreign.namespace())
                .bind(foreign.key())
                .execute(&mut *conn)
                .await?;
        }
        self.seen
            .lock()
            .expect("stager record lock")
            .extend_from_slice(impacts);
        if self.fail {
            return Err(br_util_directory::DirectoryError::Stager(
                "the adopter staging table refused the write".into(),
            ));
        }
        Ok(())
    }
}

pub fn impacts_for(seen: &Arc<Mutex<Vec<Impact>>>, id: Uuid) -> usize {
    seen.lock()
        .expect("record lock")
        .iter()
        .filter(|impact| foreign_ref(impact).key() == id.to_string())
        .count()
}

pub fn foreign_ref(impact: &Impact) -> &ForeignRef {
    match impact {
        Impact::ForeignChanged { foreign } => foreign,
        other => panic!("unexpected impact variant: {other:?}"),
    }
}
