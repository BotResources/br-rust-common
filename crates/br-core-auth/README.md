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
        claims: PassportClaims,
    },
    Service {
        service_account_id: Uuid,
        claims: PassportClaims,
    },
}
```

The variant fields are **private** — outside crates cannot build a `Passport`
from raw fields, so a non-canonical passport (e.g. `claims: null`) is
unrepresentable. Construct only through the canonical constructors:

| Constructor | Builds |
|---|---|
| `Passport::human(user_id, is_super_admin, is_active, auth_method, impersonator, claims)` | a `Human` passport |
| `Passport::service(service_account_id, claims)` | a `Service` passport |
| `.with_impersonator(admin_id)` | sets the impersonator on a `Human` (no-op on `Service`) |

`claims` is a `PassportClaims` — a newtype over a JSON **object** (see below);
it cannot hold a scalar, array, or null. Deserialization is **strict**: an
unknown top-level field is rejected, and `claims` must deserialize from a JSON
object (an explicit `null` or a non-object value is rejected). `AuthMethod` and
`BearerTokenEntry` reject unknown fields too. This is a security DTO shared by
every service, so a contract mismatch fails loud; the wire format of a *valid*
passport is unchanged.

Accessors that work uniformly over both variants:

| Method | Returns | Notes |
|---|---|---|
| `actor_id()` | `Uuid` | `user_id` (Human) or `service_account_id` (Service). For an impersonated Human, this is the impersonated user — use `impersonator_id()` for the real admin. |
| `user_id()` | `Option<Uuid>` | `Some` for `Human`, `None` for `Service`. |
| `service_account_id()` | `Option<Uuid>` | `Some` for `Service`, `None` for `Human`. |
| `is_super_admin()` | `bool` | Always `false` for `Service`. |
| `is_active()` | `bool` | Always `true` for `Service`. |
| `auth_method()` | `Option<&AuthMethod>` | `None` for `Service`. |
| `is_pat()` | `bool` | True only for `Human` with `AuthMethod::Pat { .. }`. |
| `is_impersonating()` | `bool` | True only for `Human` with `impersonator: Some(_)`. |
| `impersonator_id()` | `Option<Uuid>` | The admin's UUID when impersonating, `None` otherwise. |
| `claims()` | `&PassportClaims` | The extra claims bag (a JSON object). |
| `claim::<T>(key)` | `Option<T>` | Typed extraction of a single claim via `serde_json`. |
| `scopes()` | `Vec<ScopeKey>` | The granted scopes, parsed from the `scopes` claim. |
| `has_scope(&ScopeKey)` | `bool` | Whether a given scope is granted. |

### `PassportClaims` (newtype)

A newtype wrapping a private `serde_json::Map<String, Value>`. It serializes as
a JSON object and **rejects any non-object** on deserialization (null, array,
scalar → error), so the variants can never carry a non-object claims bag.

| Method | Returns | Notes |
|---|---|---|
| `PassportClaims::new()` | `Self` | Empty claims. |
| `PassportClaims::from_map(map)` | `Self` | Wrap an existing object map. |
| `PassportClaims::from_value(value)` | `Result<Self, Value>` | Fallible: `Err(value)` if not a JSON object. |
| `get(&str)` | `Option<&Value>` | Read one claim. |
| `iter()` | `serde_json::map::Iter` | Iterate entries. |
| `is_empty()` / `len()` | `bool` / `usize` | Size. |

### Granted scopes (Passport ↔ `ScopeKey`)

```rust
pub const SCOPES_CLAIM_KEY: &str = "scopes";

impl Passport {
    pub fn scopes(&self) -> Vec<ScopeKey>;
    pub fn has_scope(&self, scope: &ScopeKey) -> bool;
}
```

The scope **grant** is the typed counterpart of the scope **declaration**
(`br_core_scope::ScopeKey`, re-exported here). A service declares its scopes as
`ScopeKey` and authorizes a request by reading the caller's grant as `ScopeKey`
— no per-service `claim::<Vec<String>>("scopes")` convention.

- **Serialized shape:** a JSON array of scope-key strings under the `scopes`
  claim, e.g. `"claims": { "scopes": ["notifier:read", "notifier:write"] }`.
  This is the existing platform convention, now a lib contract.
- **`scopes()`** parses every entry through `ScopeKey::new` and **silently skips
  any malformed string** — a bad entry never widens access, it simply is not a
  grant. A `scopes` claim that is absent or not a string array yields an empty
  `Vec`.
- **`has_scope`** compares the requested `ScopeKey` against the raw claim
  strings; since a `ScopeKey` is always well-formed, a malformed neighbouring
  entry can never match. Fail-closed by construction.

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

### `PassportBuilder` (feature `test-support`)

```rust
let passport = PassportBuilder::new()
    .user_id(user_uuid)
    .super_admin(true)
    .active(true)
    .pat(token_id)
    .impersonator(admin_uuid)
    .claim("org_id", "acme")
    .claims([("a", json!(1)), ("b", json!(2))])
    .build();          // -> Passport::Human

let service = PassportBuilder::new().claim("name", "ci-bot").build_service();
```

A fluent builder for forging a `Passport` in tests, e2e harnesses, and gateway
examples. Co-located with `Passport` so it tracks every field change with zero
drift. Defaults: a fresh UUIDv7 `user_id`, non-super-admin, active, `Jwt`. It is
**policy-free** — claim keys (`scopes`, `org_id`, …) are set through the generic
`claim` / `claims`, never baked in.

Gated behind the **`test-support`** feature so it never reaches a production
binary; enable it as a dev-dependency:

```toml
[dev-dependencies]
br-core-auth = { git = "...", package = "br-core-auth", tag = "v0.11.1", features = ["test-support"] }
```

## Usage

```rust
use br_core_auth::{AuthMethod, Passport, PassportClaims, PassportHeader};
use uuid::Uuid;

// Producing side (e.g. svc-identity):
let passport = Passport::human(
    Uuid::new_v4(),
    false,
    true,
    AuthMethod::Jwt,
    None,
    PassportClaims::from_value(serde_json::json!({ "org_id": "..." })).unwrap(),
);
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
br-core-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "v0.11.1" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
