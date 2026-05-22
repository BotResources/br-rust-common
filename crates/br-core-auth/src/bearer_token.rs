use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Canonical KV-key derivation for the `bearer_tokens` NATS KV bucket:
/// lowercase-hex SHA-256 of the plaintext bearer token.
///
/// The plaintext token is never stored; only this hash is used as the lookup
/// key. Issuance hashes the freshly-generated token to write the entry, and
/// every authenticated request hashes the inbound token to look the entry up.
/// Both sides MUST go through this function so the hashing stays in lockstep.
pub fn bearer_token_key(plaintext: &str) -> String {
    let digest = Sha256::digest(plaintext.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut out, "{byte:02x}").expect("writing to String never fails");
    }
    out
}

/// Value stored in the `bearer_tokens` NATS KV bucket under the key returned by
/// [`bearer_token_key`].
///
/// Carries the minimum identity needed to resolve a PAT into a `Passport`:
/// - `email` — the owning user (the issuer of the PAT)
/// - `token_id` — the PAT's stable identifier, used for audit/revocation and
///   surfaced on `Passport::Human` via `AuthMethod::Pat { token_id }`
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BearerTokenEntry {
    pub email: String,
    pub token_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── bearer_token_key ─────────────────────────────────

    #[test]
    fn key_is_64_lowercase_hex_chars() {
        let k = bearer_token_key("pat_abcdef");
        assert_eq!(k.len(), 64);
        assert!(
            k.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn key_matches_known_sha256_vector() {
        // SHA-256("abc") — canonical NIST test vector.
        assert_eq!(
            bearer_token_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn key_of_empty_string_is_sha256_empty() {
        assert_eq!(
            bearer_token_key(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn key_is_deterministic() {
        assert_eq!(
            bearer_token_key("same-token"),
            bearer_token_key("same-token")
        );
    }

    #[test]
    fn key_differs_for_different_inputs() {
        assert_ne!(bearer_token_key("token-a"), bearer_token_key("token-b"));
    }

    #[test]
    fn key_is_case_sensitive_on_input() {
        assert_ne!(bearer_token_key("Token"), bearer_token_key("token"));
    }

    // ─── BearerTokenEntry serde ───────────────────────────

    #[test]
    fn entry_roundtrip() {
        let e = BearerTokenEntry {
            email: "alice@example.com".to_string(),
            token_id: Uuid::from_u128(42),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: BearerTokenEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn entry_serializes_with_expected_fields() {
        let e = BearerTokenEntry {
            email: "bob@example.com".to_string(),
            token_id: Uuid::from_u128(7),
        };
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["email"], "bob@example.com");
        assert_eq!(v["token_id"], Uuid::from_u128(7).to_string());
    }

    #[test]
    fn entry_deserializes_from_json() {
        let v = json!({
            "email": "carol@example.com",
            "token_id": "00000000-0000-0000-0000-00000000007b",
        });
        let e: BearerTokenEntry = serde_json::from_value(v).unwrap();
        assert_eq!(e.email, "carol@example.com");
        assert_eq!(e.token_id, Uuid::from_u128(123));
    }

    #[test]
    fn entry_rejects_missing_email() {
        let v = json!({"token_id": "00000000-0000-0000-0000-000000000001"});
        assert!(serde_json::from_value::<BearerTokenEntry>(v).is_err());
    }

    #[test]
    fn entry_rejects_missing_token_id() {
        let v = json!({"email": "x@y.z"});
        assert!(serde_json::from_value::<BearerTokenEntry>(v).is_err());
    }
}
