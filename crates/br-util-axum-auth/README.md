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
| `passport_header_middleware` | `async fn(Request<Body>, Next) -> Response` | Reads `X-Passport`, decodes via `Passport::from_header`, inserts the `Passport` as a request extension, then forwards to the next layer. Refuses with a plain-text body. |
| `passport_header_graphql_middleware` | `async fn(Request<Body>, Next) -> Response` | Identical decode path and identical acceptance behavior; refuses with a GraphQL-shaped JSON body instead. Opt-in — pick one or the other per router, they are siblings, not a replacement. |

Response semantics:

| Condition | `passport_header_middleware` | `passport_header_graphql_middleware` |
|---|---|---|
| Header missing, empty, non-UTF8, or malformed (bad base64 / bad JSON / wrong shape) | `401` · `text/plain; charset=utf-8` · body `unauthorized` | `401` · `application/json` · body below |
| Header valid | Continues; `request.extensions().get::<Passport>()` returns `Some(...)`. | Identical. |

The GraphQL refusal body is exactly these bytes, on every rejection cause:

```json
{"errors":[{"message":"unauthorized","extensions":{"code":"UNAUTHENTICATED"}}]}
```

`UNAUTHENTICATED` is the canonical wire string of
[`br-util-graphql`](../br-util-graphql/README.md)'s
`ErrorCode::Unauthenticated`. That crate owns the contract; this one hardcodes
the string rather than depending on it, so a service mounting the middleware
does not pull `async-graphql` in. `br-util-graphql` is a **dev**-dependency
here, and a unit test deserializes the refusal body and asserts its
`extensions.code` still equals `ErrorCode::Unauthenticated.as_str()` — drift
between the two crates fails the build, it does not ship.

Every rejection returns the **same** opaque 401: for whichever middleware is
mounted, all four rejection causes render a byte-identical body, so the
response is not a validation oracle. The precise cause (which check failed) goes to
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

For a GraphQL endpoint, mount the sibling instead so an unauthenticated call
gets a body its client can parse:

```rust
use axum::routing::post;
use br_util_axum_auth::passport_header_graphql_middleware;

let app = Router::new()
    .route("/graphql", post(graphql_handler))
    .layer(middleware::from_fn(passport_header_graphql_middleware));
```

To make a route public (skip the middleware), put it on a separate `Router`
that doesn't carry the layer and merge them at the top level.

Add to `Cargo.toml`:

```toml
[dependencies]
br-util-axum-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-axum-auth", tag = "v1.2.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
