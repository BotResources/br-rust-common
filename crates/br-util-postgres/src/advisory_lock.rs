use sqlx::PgConnection;

pub(crate) async fn lock_transaction_scope(
    connection: &mut PgConnection,
    key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(key)
        .execute(connection)
        .await
        .map(|_| ())
}
