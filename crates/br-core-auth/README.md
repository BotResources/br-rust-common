# br-core-auth

`Passport` identity DTO and `X-Passport` header codec used across all
BotResources services.

**Purpose.** `Passport` (Human | Service) is the identity representation
that `svc-identity` builds and every downstream service consumes. It is
transported between services as a base64-encoded JSON value in the
`X-Passport` HTTP header.

**When to use.** Any service that authenticates incoming requests (receives
`X-Passport`) or propagates identity downstream.

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

### `PassportError`

Returned by `from_header` and friends. Inspect via `Debug` /
`thiserror::Error` impl.

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
br-core-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "br-core-auth-v0.4.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
