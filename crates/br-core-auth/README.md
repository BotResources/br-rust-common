# br-core-auth

`Passport` identity DTO + `X-Passport` header codec, plus the canonical PAT
bearer-token contract, shared across all BotResources services.

**Purpose.** `Passport` (Human | Service) is the identity representation
that `svc-identity` builds and every downstream service consumes. It is
transported between services as a base64-encoded JSON value in the
`X-Passport` HTTP header. For PAT-authenticated requests, this crate also
defines the canonical key/value shape (`bearer_token_key` +
`BearerTokenEntry`) that every service uses to resolve a PAT against the
shared `bearer_tokens` NATS KV bucket.

**When to use.** Any service that authenticates incoming requests (receives
`X-Passport`), propagates identity downstream, or resolves PATs against
the shared bearer-token KV.

**When not to use.** You are inside a bounded context that has already
extracted the identity from the Passport into its own domain types. Don't
pass `Passport` through the domain layer.

## What's inside

### `Passport` (enum)

```rust
pub enum Passport {
    Human {
        user_id: Uuid,
        is_super_admin: bool,
        is_active: bool,
        auth_method: AuthMethod,
        impersonator: Option<Uuid>, // serde(default)
        claims: serde_json::Value,
    },
    Service {
        service_account_id: Uuid,
        claims: serde_json::Value,
    },
}
```

Deserialization is **strict**: an unknown top-level field is rejected, and
`claims` must be a JSON object (an explicit `null` or a non-object value is
rejected). `AuthMethod` and `BearerTokenEntry` reject unknown fields too. This
is a security DTO shared by every service, so a contract mismatch fails loud;
the wire format of a *valid* passport is unchanged.

Accessors that work uniformly over both variants:

| Method | Returns | Notes |
|---|---|---|
| `actor_id()` | `Uuid` | `user_id` (Human) or `service_account_id` (Service). For an impersonated Human, this is the impersonated user — use `impersonator_id()` for the real admin. |
| `is_super_admin()` | `bool` | Always `false` for `Service`. |
| `is_active()` | `bool` | Always `true` for `Service`. |
| `auth_method()` | `Option<&AuthMethod>` | `None` for `Service`. |
| `is_pat()` | `bool` | True only for `Human` with `AuthMethod::Pat { .. }`. |
| `is_impersonating()` | `bool` | True only for `Human` with `impersonator: Some(_)`. |
| `impersonator_id()` | `Option<Uuid>` | The admin's UUID when impersonating, `None` otherwise. |
| `claims()` | `&serde_json::Value` | Raw extra claims bag. |
| `claim::<T>(key)` | `Option<T>` | Typed extraction of a single claim via `serde_json`. |

### `AuthMethod` (enum)

```rust
pub enum AuthMethod {
    Jwt,
    Pat { token_id: Uuid },
}
```

Helpers: `is_pat()`, `pat_token_id() -> Option<Uuid>`. Only attached to
`Human` passports; `Service` passports do not carry an auth method.

### `PassportHeader` (trait, implemented for `Passport`)

```rust
pub trait PassportHeader: Sized {
    fn to_header(&self) -> String;
    fn from_header(header: &str) -> Result<Self, PassportError>;
}
```

`to_header` produces base64(JSON). `from_header` rejects invalid base64,
invalid JSON, and JSON that doesn't match the `Passport` enum shape.

`X-Passport` is trustworthy only because the gateway strips any client-supplied
copy and re-injects the resolved one (and NetworkPolicy blocks direct access);
`from_header` *decodes* the header, it does not authenticate its origin.

### `PassportError`

Returned by `from_header` and friends. Inspect via `Debug` /
`thiserror::Error` impl.

### `bearer_token_key` + `BearerTokenEntry` (PAT bearer-token contract)

```rust
pub fn bearer_token_key(plaintext: &str) -> String;

pub struct BearerTokenEntry {
    pub email: String,
    pub token_id: Uuid,
}
```

Canonical contract for the shared `bearer_tokens` NATS KV bucket used to
resolve a PAT into a `Passport`:

- `bearer_token_key` derives the KV key as lowercase-hex SHA-256 of the
  plaintext bearer token. **Issuance** hashes a freshly-generated token to
  write the entry; **authentication** hashes the inbound token to look the
  entry up. Both sides MUST go through this function so the hashing stays
  in lockstep. The plaintext token is never stored.
- `BearerTokenEntry` is the value stored under that key. `token_id` is the
  PAT's stable identifier — the same UUID surfaced on `Passport::Human`
  via `AuthMethod::Pat { token_id }` for audit / revocation.

PATs are server-issued high-entropy random secrets, so a fast hash is the
right tool here (key derivation, not password storage). Do **not** adapt
`bearer_token_key` for human-chosen secrets — those need argon2/bcrypt.

### Session cookie (`session_cookie_name` + `extract_session_id`)

```rust
pub fn session_cookie_name(secure: bool) -> &'static str;
pub fn extract_session_id(cookie_header: &str, secure: bool) -> Option<&str>;
```

Canonical session-cookie contract between svc-auth (producer) and
svc-identity (consumer). `session_cookie_name` returns `__Host-session_id`
in production (`secure = true`) or `session_id` in local dev; both sides go
through it so the name stays in lockstep.

`extract_session_id` parses a raw `Cookie` header and returns the session ID.
Extraction is **fail-closed on anything ambiguous**:

- **Exact, case-sensitive name match** (RFC 6265). A variant-case name such as
  `SESSION_ID=…` is rejected. This is load-bearing for the `__Host-` prefix:
  the browser's `Secure; Path=/; no Domain` guarantees apply only to the
  exact-case prefix, so a variant-case `__HOST-session_id` would be a cookie
  the browser never constrained.
- **Duplicates rejected.** If the exact name appears more than once in the
  header, the value is ambiguous (the signature of a cookie-tossing /
  prefix-injection attempt) and `extract_session_id` returns `None` rather
  than picking a winner.

## Usage

```rust
use br_core_auth::{AuthMethod, Passport, PassportHeader};
use uuid::Uuid;

// Producing side (e.g. svc-identity):
let passport = Passport::Human {
    user_id: Uuid::new_v4(),
    is_super_admin: false,
    is_active: true,
    auth_method: AuthMethod::Jwt,
    impersonator: None,
    claims: serde_json::json!({ "org_id": "..." }),
};
let header_value: String = passport.to_header();

// Consuming side (gateway, middleware, or service):
let received = Passport::from_header(&header_value)?;
let user = received.actor_id();
if received.is_impersonating() {
    audit_log(received.impersonator_id().unwrap(), user);
}
```

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "br-core-auth-v0.6.2" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
