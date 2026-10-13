use super::*;
use crate::fixtures::with_userinfo;

#[test]
fn localhost_always_allowed() {
    assert!(validate_database_tls("postgres://localhost/db").is_ok());
}

#[test]
fn ipv6_loopback_always_allowed() {
    assert!(validate_database_tls("postgres://user@[::1]:5432/db").is_ok());
}

#[test]
fn unparseable_host_fails_closed() {
    assert!(matches!(
        validate_database_tls("postgres://"),
        Err(PostgresError::Config(_))
    ));
}

#[test]
fn remote_without_tls_rejected() {
    assert!(validate_database_tls("postgres://db.example.com/db").is_err());
}

#[test]
fn remote_with_tls_accepted() {
    assert!(validate_database_tls("postgres://db.example.com/db?sslmode=require").is_ok());
}

#[test]
fn remote_without_tls_rejected_regardless_of_sslmode_off() {
    assert!(validate_database_tls("postgres://db.example.com/db?sslmode=disable").is_err());
}

#[test]
fn require_sslmode_passes_tls_validation() {
    assert!(validate_database_tls("postgres://db.example.com/db?sslmode=require").is_ok());
}

#[test]
fn sslmode_among_other_params_is_honored() {
    assert!(
        validate_database_tls(
            "postgres://db.example.com/db?connect_timeout=10&sslmode=verify-full&application_name=test",
        )
        .is_ok()
    );
}

#[test]
fn absent_sslmode_defaults_to_prefer_and_rejects() {
    assert!(validate_database_tls("postgres://db.example.com/db?connect_timeout=10").is_err());
    assert!(validate_database_tls("postgres://db.example.com/db").is_err());
}

#[test]
fn malformed_url_fails_closed() {
    assert!(matches!(
        validate_database_tls("postgres://db.example.com/db?sslmode=not-a-mode"),
        Err(PostgresError::Config(_))
    ));
}

#[test]
fn loopback_authority_with_host_override_is_rejected() {
    assert!(matches!(
        validate_database_tls("postgres://localhost/db?host=evil.example.com"),
        Err(PostgresError::Config(_))
    ));
}

#[test]
fn loopback_authority_with_hostaddr_override_is_rejected() {
    assert!(matches!(
        validate_database_tls("postgres://127.0.0.1/db?hostaddr=8.8.8.8"),
        Err(PostgresError::Config(_))
    ));
}

#[test]
fn percent_encoded_host_override_is_rejected() {
    assert!(matches!(
        validate_database_tls("postgres://localhost/db?%68ost=evil.example.com"),
        Err(PostgresError::Config(_))
    ));
}

#[test]
fn other_query_params_do_not_trip_the_override_guard() {
    assert!(validate_database_tls("postgres://localhost/db?application_name=svc").is_ok());
}

#[test]
fn duplicate_sslmode_require_then_disable_rejects_tls() {
    assert!(
        validate_database_tls("postgres://db.example.com/db?sslmode=require&sslmode=disable")
            .is_err()
    );
}

#[test]
fn ssl_mode_hyphenated_passes_tls_validation() {
    assert!(validate_database_tls("postgres://db.example.com/db?ssl-mode=verify-full").is_ok());
}

#[test]
fn uppercase_sslmode_passes_tls_validation() {
    assert!(validate_database_tls("postgres://db.example.com/db?sslmode=REQUIRE").is_ok());
}

#[test]
fn mixed_case_ssl_mode_passes_tls_validation() {
    assert!(validate_database_tls("postgres://db.example.com/db?ssl-mode=Verify-Ca").is_ok());
}

#[test]
fn remote_host_hidden_behind_an_at_localhost_tail_is_not_loopback() {
    for dsn in [
        "postgres://evil.example.com/db?x=a@localhost",
        "postgres://evil.example.com?x=a@127.0.0.1",
        "postgres://evil.example.com/a@localhost/db",
        "postgres://evil.example.com/@localhost",
        "postgres://evil.example.com/db#a@localhost",
        "postgres://evil.example.com#@[::1]",
        "postgres://evil.example.com/db?x=%40localhost&y=a@localhost",
    ] {
        assert!(
            matches!(validate_database_tls(dsn), Err(PostgresError::Config(_))),
            "{dsn} must require TLS"
        );
    }
}

#[test]
fn remote_host_hidden_behind_an_at_localhost_tail_is_accepted_only_with_tls() {
    assert!(
        validate_database_tls("postgres://evil.example.com/db?x=a@localhost&sslmode=require")
            .is_ok()
    );
}

#[test]
fn percent_encoded_at_sign_in_userinfo_does_not_make_the_host_loopback() {
    assert!(
        validate_database_tls(&with_userinfo("us%40localhost:pw", "evil.example.com/db")).is_err()
    );
    assert!(
        validate_database_tls(&with_userinfo("user:p%40localhost", "evil.example.com/db")).is_err()
    );
    assert!(
        validate_database_tls(&with_userinfo("user:pa@localhost", "evil.example.com/db")).is_err()
    );
}

#[test]
fn loopback_in_credentials_does_not_make_the_host_loopback() {
    assert!(validate_database_tls(&with_userinfo("localhost:pw", "evil.example.com/db")).is_err());
    assert!(validate_database_tls("postgres://127.0.0.1@evil.example.com/db").is_err());
}

#[test]
fn loopback_prefix_in_a_host_list_is_not_loopback() {
    assert!(validate_database_tls("postgres://localhost,evil.example.com/db").is_err());
}

#[test]
fn real_loopback_with_at_sign_elsewhere_is_still_loopback() {
    assert!(
        validate_database_tls(&with_userinfo(
            "user:pw",
            "localhost/db?x=a@evil.example.com"
        ))
        .is_ok()
    );
    assert!(validate_database_tls("postgres://user@127.0.0.1:5432/db#a@evil.example.com").is_ok());
    assert!(validate_database_tls("postgres://user@[::1]/db?x=a@evil.example.com").is_ok());
}

#[test]
fn missing_or_empty_host_is_a_config_error_never_loopback() {
    for dsn in [
        "postgres://",
        "postgres:///db",
        "postgres://user@/db",
        &with_userinfo("user:pw", ":5432/db"),
        "postgres://:5432/db",
        "postgres:db",
        "postgres:?x=a@localhost",
    ] {
        assert!(
            matches!(validate_database_tls(dsn), Err(PostgresError::Config(_))),
            "{dsn} must be refused"
        );
    }
}

#[test]
fn dsn_without_a_postgres_scheme_is_a_config_error() {
    for dsn in ["localhost", "localhost/db", "http://localhost/db", ""] {
        assert!(
            matches!(validate_database_tls(dsn), Err(PostgresError::Config(_))),
            "{dsn:?} must be refused"
        );
    }
}

#[test]
fn unparseable_loopback_dsn_is_a_config_error() {
    assert!(matches!(
        validate_database_tls("postgres://localhost/db?sslmode=not-a-mode"),
        Err(PostgresError::Config(_))
    ));
    assert!(matches!(
        validate_database_tls("postgres://localhost:notaport/db"),
        Err(PostgresError::Config(_))
    ));
}

#[tokio::test]
async fn init_pool_refuses_a_hidden_remote_host_before_any_network_call() {
    let result = crate::pool::init_pool("postgres://evil.example.com/db?x=a@localhost").await;
    assert!(matches!(result, Err(PostgresError::Config(_))));
}
