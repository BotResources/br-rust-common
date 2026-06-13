# br-util-axum-auth

Axum middleware that decodes the `X-Passport` header into a typed
`Passport` request extension.

**Purpose.** Wraps `Passport::from_header` (from
[`br-core-auth`](../br-core-auth/README.md)) as an `axum::middleware::from_fn`
layer. Handlers receive a ready-to-use `Extension<Passport>` instead of
parsing the header themselves.

**When to use.** An Axum-based service receives authenticated calls (via
`svc-identity` or a gateway) and wants the Passport available as an
`axum::Extension<Passport>` on every handler.

**When not to use.** The service uses a different HTTP framework, or does
its own identity extraction (e.g. parses a JWT directly).

## What's inside

| Item | Kind | Behavior |
|---|---|---|
| `passport_header_middleware` | `async fn(Request<Body>, Next) -> Response` | Reads `X-Passport`, decodes via `Passport::from_header`, inserts the `Passport` as a request extension, then forwards to the next layer. |

Response semantics:

| Condition | Response |
|---|---|
| Header missing, empty, non-UTF8, or malformed (bad base64 / bad JSON / wrong shape) | `401 Unauthorized` — uniform opaque body `"unauthorized"` |
| Header valid | Continues; `request.extensions().get::<Passport>()` returns `Some(...)`. |

Every rejection returns the **same** opaque 401, so the response is not a
validation oracle. The precise cause (which check failed) goes to
`tracing::warn!` server-side; the header value is never logged (it may carry a
forged passport payload).

The middleware does **not** enforce any policy beyond presence and
decodability — `is_active`, `is_super_admin`, RLS, scope checks, etc. are
the handler's or a separate layer's responsibility.

**Trust boundary.** `X-Passport` is trustworthy only because the gateway
strips any client-supplied copy and re-injects the resolved one, and
NetworkPolicy blocks direct external access. This middleware *decodes* the
header — it does not authenticate its origin, so never expose a service
mounting it except behind the gateway.

## Usage

```rust
use axum::{Extension, Router, middleware, routing::get};
use br_core_auth::Passport;
use br_util_axum_auth::passport_header_middleware;

async fn me(Extension(passport): Extension<Passport>) -> String {
    format!("hello {}", passport.actor_id())
}

let app = Router::new()
    .route("/me", get(me))
    .layer(middleware::from_fn(passport_header_middleware));
```

To make a route public (skip the middleware), put it on a separate `Router`
that doesn't carry the layer and merge them at the top level.

Add to `Cargo.toml`:

```toml
[dependencies]
br-util-axum-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-axum-auth", tag = "v0.9.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
