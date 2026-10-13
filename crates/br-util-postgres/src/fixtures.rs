pub(crate) fn with_userinfo(userinfo: &str, rest: &str) -> String {
    format!("postgres://{userinfo}@{rest}")
}
