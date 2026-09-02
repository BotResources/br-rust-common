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

    pub(crate) async fn apply_single_statement(
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

#[cfg(test)]
mod tests {
    use super::*;

    use sqlx::postgres::PgPoolOptions;

    async fn probe() -> (SinkContext, String) {
        let url = br_test_support::require_test_db_url();
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect to TEST_DATABASE_URL");
        let table = br_test_support::unique_table_name();
        sqlx::query(&format!("CREATE TABLE \"{table}\" (marker text)"))
            .execute(&pool)
            .await
            .expect("create the probe table");
        (SinkContext::new(pool, None, ProgressChannel::new()), table)
    }

    async fn marker_rows(context: &SinkContext, table: &str) -> i64 {
        let (count,): (i64,) = sqlx::query_as(&format!("SELECT count(*) FROM \"{table}\""))
            .fetch_one(context.pool())
            .await
            .expect("count the probe rows");
        count
    }

    async fn discard(context: &SinkContext, table: &str) {
        sqlx::query(&format!("DROP TABLE \"{table}\""))
            .execute(context.pool())
            .await
            .expect("drop the probe table");
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
    async fn a_single_statement_apply_keeps_the_first_write_when_a_second_one_fails() {
        let (context, table) = probe().await;
        let insert = format!("INSERT INTO \"{table}\" (marker) VALUES ('first')");

        let outcome = context
            .apply_single_statement(async |conn| {
                sqlx::query(&insert).execute(&mut *conn).await?;
                sqlx::query("SELECT 1 / 0").execute(&mut *conn).await?;
                Ok(Vec::new())
            })
            .await;

        assert!(outcome.is_err(), "the failing statement surfaces");
        assert_eq!(
            marker_rows(&context, &table).await,
            1,
            "no transaction was opened: the first statement is already committed"
        );
        discard(&context, &table).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
    async fn the_transactional_apply_rolls_the_very_same_pair_back() {
        let (context, table) = probe().await;
        let insert = format!("INSERT INTO \"{table}\" (marker) VALUES ('first')");

        let outcome = context
            .apply_in_transaction(async |conn| {
                sqlx::query(&insert).execute(&mut *conn).await?;
                sqlx::query("SELECT 1 / 0").execute(&mut *conn).await?;
                Ok(Vec::new())
            })
            .await;

        assert!(outcome.is_err(), "the failing statement surfaces");
        assert_eq!(
            marker_rows(&context, &table).await,
            0,
            "the transaction rolled the first statement back with the second"
        );
        discard(&context, &table).await;
    }
}
