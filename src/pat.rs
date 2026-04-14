use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

/// Default prefix for Personal Access Tokens.
pub const DEFAULT_PAT_PREFIX: &str = "pat_";

/// Default prefix for API Keys (service account tokens).
pub const DEFAULT_API_KEY_PREFIX: &str = "key_";

/// Generate a new Personal Access Token with a custom prefix.
///
/// Returns `(plain_token, token_hash, token_prefix)`:
/// - `plain_token`: the full token string including the prefix (shown to user once)
/// - `token_hash`: SHA-256 hash of the plain token (stored in DB for lookup)
/// - `token_prefix`: first 12 chars of the plain token (for user display)
pub fn generate_pat(prefix: &str) -> (String, Vec<u8>, String) {
    generate_token(prefix, 12)
}

/// Generate a new API Key for service accounts with a custom prefix.
///
/// Returns `(plain_token, token_hash, token_prefix)`:
/// - `plain_token`: the full token string including the prefix (shown to caller once)
/// - `token_hash`: SHA-256 hash of the plain token (stored in DB for lookup)
/// - `token_prefix`: first 16 chars of the plain token (for display)
///
/// Same algorithm as `generate_pat` (32 random bytes + base64url + SHA-256), different prefix.
pub fn generate_api_key(prefix: &str) -> (String, Vec<u8>, String) {
    generate_token(prefix, 16)
}

/// Internal: generate a token with the given prefix and prefix display length.
fn generate_token(prefix: &str, prefix_len: usize) -> (String, Vec<u8>, String) {
    let mut bytes = [0u8; 32];
    rand::Fill::fill(&mut bytes, &mut rand::rng());

    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    let plain_token = format!("{prefix}{encoded}");
    let token_hash = hash_token(&plain_token);
    let token_prefix = plain_token.chars().take(prefix_len).collect();

    (plain_token, token_hash, token_prefix)
}

/// Compute SHA-256 hash of a token string.
pub fn hash_token(token: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── PAT tests ─────────────────────────────────────

    #[test]
    fn generate_pat_has_prefix() {
        let (plain, _hash, prefix) = generate_pat(DEFAULT_PAT_PREFIX);
        assert!(plain.starts_with(DEFAULT_PAT_PREFIX));
        assert!(prefix.starts_with(DEFAULT_PAT_PREFIX));
        assert_eq!(prefix.len(), 12);
    }

    #[test]
    fn generate_pat_token_length_is_consistent() {
        // pat_ (4 chars) + base64url(32 bytes) = 4 + 43 = 47 chars
        let (plain, _, _) = generate_pat(DEFAULT_PAT_PREFIX);
        assert_eq!(plain.len(), 4 + 43);
    }

    #[test]
    fn generate_pat_custom_prefix() {
        let (plain, _hash, prefix) = generate_pat("myapp_");
        assert!(plain.starts_with("myapp_"));
        assert!(prefix.starts_with("myapp_"));
        // prefix is first 12 chars
        assert_eq!(prefix.len(), 12);
    }

    #[test]
    fn hash_is_deterministic() {
        let h1 = hash_token("pat_test_token");
        let h2 = hash_token("pat_test_token");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_tokens_produce_different_hashes() {
        let (t1, h1, _) = generate_pat(DEFAULT_PAT_PREFIX);
        let (t2, h2, _) = generate_pat(DEFAULT_PAT_PREFIX);
        assert_ne!(t1, t2);
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_token_matches_generate_pat_hash() {
        let (plain, hash_from_gen, _) = generate_pat(DEFAULT_PAT_PREFIX);
        let hash_manual = hash_token(&plain);
        assert_eq!(hash_from_gen, hash_manual);
    }

    // ─── API Key tests ─────────────────────────────────

    #[test]
    fn generate_api_key_has_prefix() {
        let (plain, _hash, prefix) = generate_api_key(DEFAULT_API_KEY_PREFIX);
        assert!(plain.starts_with(DEFAULT_API_KEY_PREFIX));
        assert!(prefix.starts_with(DEFAULT_API_KEY_PREFIX));
        assert_eq!(prefix.len(), 16);
    }

    #[test]
    fn generate_api_key_token_length_is_consistent() {
        // key_ (4 chars) + base64url(32 bytes) = 4 + 43 = 47 chars
        let (plain, _, _) = generate_api_key(DEFAULT_API_KEY_PREFIX);
        assert_eq!(plain.len(), 4 + 43);
    }

    #[test]
    fn generate_api_key_custom_prefix() {
        let (plain, _hash, prefix) = generate_api_key("svc_");
        assert!(plain.starts_with("svc_"));
        assert!(prefix.starts_with("svc_"));
        assert_eq!(prefix.len(), 16);
    }

    #[test]
    fn generate_api_key_hash_matches_manual_hash() {
        let (plain, hash_from_gen, _) = generate_api_key(DEFAULT_API_KEY_PREFIX);
        let hash_manual = hash_token(&plain);
        assert_eq!(hash_from_gen, hash_manual);
    }

    #[test]
    fn different_api_keys_produce_different_hashes() {
        let (t1, h1, _) = generate_api_key(DEFAULT_API_KEY_PREFIX);
        let (t2, h2, _) = generate_api_key(DEFAULT_API_KEY_PREFIX);
        assert_ne!(t1, t2);
        assert_ne!(h1, h2);
    }

    #[test]
    fn api_key_and_pat_have_different_prefixes() {
        let (pat, _, _) = generate_pat(DEFAULT_PAT_PREFIX);
        let (api_key, _, _) = generate_api_key(DEFAULT_API_KEY_PREFIX);
        assert!(pat.starts_with("pat_"));
        assert!(api_key.starts_with("key_"));
        assert_ne!(pat[..4], api_key[..4]);
    }

    #[test]
    fn hash_token_sha256_produces_32_bytes() {
        let hash = hash_token("any_input");
        assert_eq!(hash.len(), 32);
    }
}
