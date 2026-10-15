use std::future::Future;
use std::time::Duration;

use sqlx::error::DatabaseError;

use crate::error::PostgresError;

const MAX_ATTEMPTS: u32 = 6;
const BACKOFF_STEP: Duration = Duration::from_millis(25);

const DUPLICATE_OBJECT: &str = "42710";
const UNIQUE_VIOLATION: &str = "23505";
const INTERNAL_ERROR: &str = "XX000";

fn is_concurrent_role_ddl(error: &dyn DatabaseError) -> bool {
    matches!(
        error.code().as_deref(),
        Some(DUPLICATE_OBJECT | UNIQUE_VIOLATION | INTERNAL_ERROR)
    )
}

pub(crate) async fn retry_concurrent_role_ddl<F, Fut>(mut attempt: F) -> Result<(), PostgresError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), sqlx::Error>>,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match attempt().await {
            Ok(()) => return Ok(()),
            Err(sqlx::Error::Database(error))
                if attempts < MAX_ATTEMPTS && is_concurrent_role_ddl(error.as_ref()) =>
            {
                tokio::time::sleep(BACKOFF_STEP * attempts).await;
            }
            Err(error) => return Err(PostgresError::Db(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::fmt;
    use std::sync::atomic::{AtomicU32, Ordering};

    use sqlx::error::ErrorKind;

    use super::*;

    #[derive(Debug)]
    struct StubDatabaseError {
        code: Option<&'static str>,
    }

    impl fmt::Display for StubDatabaseError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("stub database error")
        }
    }

    impl std::error::Error for StubDatabaseError {}

    impl DatabaseError for StubDatabaseError {
        fn message(&self) -> &str {
            "stub database error"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            self.code.map(Cow::Borrowed)
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    fn database_error(code: Option<&'static str>) -> sqlx::Error {
        sqlx::Error::Database(Box::new(StubDatabaseError { code }))
    }

    async fn always_failing_with(
        make_error: impl Fn() -> sqlx::Error,
    ) -> (Result<(), PostgresError>, u32) {
        let calls = AtomicU32::new(0);
        let result = retry_concurrent_role_ddl(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            let error = make_error();
            async move { Err(error) }
        })
        .await;
        (result, calls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn each_concurrent_ddl_code_is_retried_until_the_sixth_attempt() {
        for code in [DUPLICATE_OBJECT, UNIQUE_VIOLATION, INTERNAL_ERROR] {
            let (result, calls) = always_failing_with(|| database_error(Some(code))).await;
            assert!(matches!(result, Err(PostgresError::Db(_))), "code={code}");
            assert_eq!(calls, MAX_ATTEMPTS, "code={code}");
        }
    }

    #[tokio::test]
    async fn a_retried_call_that_then_succeeds_stops_retrying() {
        let calls = AtomicU32::new(0);
        let result = retry_concurrent_role_ddl(|| {
            let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if attempt < 3 {
                    Err(database_error(Some(DUPLICATE_OBJECT)))
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn any_other_sqlstate_returns_at_once() {
        for code in ["23503", "40001", "42501", "42P01", "57014", "08006"] {
            let (result, calls) = always_failing_with(|| database_error(Some(code))).await;
            assert!(matches!(result, Err(PostgresError::Db(_))), "code={code}");
            assert_eq!(calls, 1, "code={code}");
        }
    }

    #[tokio::test]
    async fn a_database_error_without_a_code_returns_at_once() {
        let (result, calls) = always_failing_with(|| database_error(None)).await;
        assert!(matches!(result, Err(PostgresError::Db(_))));
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn pool_closed_returns_at_once() {
        let (result, calls) = always_failing_with(|| sqlx::Error::PoolClosed).await;
        assert!(matches!(
            result,
            Err(PostgresError::Db(sqlx::Error::PoolClosed))
        ));
        assert_eq!(calls, 1);
    }
}
