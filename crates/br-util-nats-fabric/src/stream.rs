pub const INTEGRATION_CMD: &str = "INTEGRATION_CMD";
pub const INTEGRATION_EVT: &str = "INTEGRATION_EVT";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_constants_are_the_frozen_names() {
        assert_eq!(INTEGRATION_CMD, "INTEGRATION_CMD");
        assert_eq!(INTEGRATION_EVT, "INTEGRATION_EVT");
    }
}
