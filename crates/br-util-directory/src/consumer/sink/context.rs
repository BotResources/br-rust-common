use std::sync::Arc;

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::consumer::progress::ProgressChannel;
use crate::error::DirectoryError;
use crate::impact::{ForeignRef, Impact, ImpactStager};

#[derive(Clone)]
pub(crate) struct SinkContext {
    pool: PgPool,
    stager: Option<Arc<dyn ImpactStager>>,
    progress: ProgressChannel,
}

impl SinkContext {
    pub(crate) fn new(
        pool: PgPool,
        stager: Option<Arc<dyn ImpactStager>>,
        progress: ProgressChannel,
    ) -> Self {
        Self {
            pool,
            stager,
            progress,
        }
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) async fn stage_change(
        &self,
        conn: &mut PgConnection,
        namespace: &str,
        id: Uuid,
    ) -> Result<(), DirectoryError> {
        let Some(stager) = self.stager.as_ref() else {
            return Ok(());
        };
        let impacts = [Impact::ForeignChanged {
            foreign: ForeignRef::new(namespace, &id.to_string())?,
        }];
        stager.stage_in(conn, &impacts).await
    }

    pub(crate) fn record_change(&self) {
        self.progress.bump();
    }
}
