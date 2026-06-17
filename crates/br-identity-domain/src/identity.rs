use br_core_scope::ServiceKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectedIdentity {
    Service(ServiceKey),
    Unrepresentable { raw: String },
}

impl RejectedIdentity {
    pub fn from_raw_key(raw: &str) -> Self {
        match ServiceKey::new(raw) {
            Ok(service) => Self::Service(service),
            Err(_) => Self::Unrepresentable {
                raw: raw.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_raw_key_is_a_typed_service_identity() {
        assert_eq!(
            RejectedIdentity::from_raw_key("notifier"),
            RejectedIdentity::Service(ServiceKey::new("notifier").unwrap())
        );
    }

    #[test]
    fn an_invalid_raw_key_is_a_typed_unrepresentable_identity_not_a_default() {
        assert_eq!(
            RejectedIdentity::from_raw_key("NOPE"),
            RejectedIdentity::Unrepresentable {
                raw: "NOPE".to_string(),
            }
        );
    }
}
