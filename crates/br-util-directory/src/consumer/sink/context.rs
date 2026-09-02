use std::sync::Arc;

use sqlx::{PgConnection, PgPool};

use crate::consumer::progress::ProgressChannel;
use crate::error::DirectoryError;
use crate::impact::{Impact, ImpactStager};

pub(crate) type SinkOutcome = Result<Vec<Impact>, DirectoryError>;

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

    pub(crate) fn stages_impacts(&self) -> bool {
        self.stager.is_some()
    }

    pub(crate) async fn apply(
        &self,
        write: impl AsyncFnOnce(&mut PgConnection) -> SinkOutcome,
    ) -> Result<(), DirectoryError> {
        if self.stager.is_some() {
            return self.apply_in_transaction(write).await;
        }
        let mut conn = self.pool.acquire().await?;
        let impacts = write(&mut conn).await?;
        self.settle(&impacts);
        Ok(())
    }

    pub(crate) async fn apply_in_transaction(
        &self,
        write: impl AsyncFnOnce(&mut PgConnection) -> SinkOutcome,
    ) -> Result<(), DirectoryError> {
        let mut tx = self.pool.begin().await?;
        let impacts = write(&mut tx).await?;
        if let Some(stager) = self.stager.as_ref()
            && !impacts.is_empty()
        {
            stager.stage_in(&mut tx, &impacts).await?;
        }
        tx.commit().await?;
        self.settle(&impacts);
        Ok(())
    }

    fn settle(&self, impacts: &[Impact]) {
        if !impacts.is_empty() {
            self.progress.bump();
        }
    }
}
