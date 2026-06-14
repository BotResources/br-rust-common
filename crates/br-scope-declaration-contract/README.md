# br-scope-declaration-contract

The single source of the **identity service-scope declaration** wire coordinates.
A generic service declares its scopes to Identity over the integration bus; the
subject strings, version, and command/event type names of that exchange are a
published contract that **two** crates must agree on: the producer-side handshake
(`br-util-scope-declaration`) and the Identity-side publisher (`br-identity-app`).
This crate holds those coordinates once so the two sides cannot drift.

Domain-light by construction: it depends only on `br-core-integration` (for the
subject builder), holds no aggregate, no event payloads, no policy.

## What's inside

| Item | Kind | Value |
|---|---|---|
| `BC` | `&str` | `"identity"` |
| `AGGREGATE` | `&str` | `"service_scope"` |
| `VERSION` | `u8` | `1` |
| `COMMAND_NAME` | `&str` | `"declare"` |
| `ACCEPTED` / `REJECTED` | `&str` | confirmation event names |
| `command_subject()` | `fn() -> Result<String, SubjectError>` | `identity.cmd.service_scope.declare.v1` |
| `event_subject(name)` | `fn(&str) -> Result<String, SubjectError>` | `identity.evt.service_scope.<name>.v1` |
| `accepted_subject()` / `rejected_subject()` | `fn() -> Result<String, SubjectError>` | the two confirmation subjects |
| `command_type()` | `fn() -> String` | `service_scope.declare` |
| `event_type(name)` | `fn(&str) -> String` | `service_scope.<name>` |

The canonical wire-string assertion lives in this crate's tests; the consuming
crates assert only their own composition logic.

## Install

```toml
[dependencies]
br-scope-declaration-contract = { git = "https://github.com/BotResources/br-rust-common", package = "br-scope-declaration-contract", tag = "v0.11.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
