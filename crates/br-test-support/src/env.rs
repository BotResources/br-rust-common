pub fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

pub fn test_tls_db_url() -> Option<String> {
    std::env::var("TEST_TLS_DATABASE_URL").ok()
}

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

pub fn require_nats_url() -> String {
    nats_url().expect("NATS_URL is required for this ignored suite")
}

pub fn require_test_db_url() -> String {
    test_db_url().expect("TEST_DATABASE_URL is required for this ignored suite")
}

pub fn require_test_tls_db_url() -> String {
    test_tls_db_url().expect("TEST_TLS_DATABASE_URL is required for this ignored suite")
}
