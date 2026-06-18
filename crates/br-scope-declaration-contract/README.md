# br-scope-declaration-contract

The single source of the **identity service-scope declaration** wire coordinates.
A generic service declares its scopes to Identity over the integration bus; the
typed command/event coordinates, version, and command/event type names of that
exchange are a published contract that **two** crates must agree on: the
producer-side handshake (`br-util-scope-declaration`) and the Identity-side
publisher (`br-identity-app`). This crate holds those coordinates once so the
two sides cannot drift.

The coordinates are the **typed** `CommandCoords` / `EventCoords` from
`br-core-integration`, not freestyle subject strings: the contract exposes the
validated tuple `(receiver/producer, aggregate, verb/fact, version)`, and the
NATS Fabric (`br-util-nats-fabric`) owns the `integration.…` subject rendering.
On the Fabric grammar these render as
`integration.cmd.identity.service_scope.declare.v1`,
`integration.evt.identity.service_scope.accepted.v1`, and `…rejected.v1`.

Domain-light by construction: it depends only on `br-core-integration` (for the
coordinate types), holds no aggregate, no event payloads, no policy.

## What's inside

| Item | Kind | Value |
|---|---|---|
| `BC` | `&str` | `"identity"` |
| `AGGREGATE` | `&str` | `"service_scope"` |
| `VERSION` | `u8` | `1` |
| `COMMAND_NAME` | `&str` | `"declare"` |
| `ACCEPTED` / `REJECTED` | `&str` | confirmation fact names |
| `UNREPRESENTABLE_SERVICE` | `&str` | `"unrepresentable_service"` — service-key sentinel emitted on the `rejected` fact when the declared manifest key is unparsable |
| `declare_command_coords()` | `fn() -> Result<CommandCoords, CoordError>` | receiver=`identity`, aggregate=`service_scope`, verb=`declare`, v1 |
| `accepted_event_coords()` / `rejected_event_coords()` | `fn() -> Result<EventCoords, CoordError>` | producer=`identity`, aggregate=`service_scope`, fact=`accepted`/`rejected`, v1 |
| `command_type()` | `fn() -> String` | `service_scope.declare` |
| `event_type(name)` | `fn(&str) -> String` | `service_scope.<name>` |

The canonical coordinate assertion lives in this crate's tests; the Fabric owns
the wire-string assertion; the consuming crates assert only their own
composition logic.

## Install

```toml
[dependencies]
br-scope-declaration-contract = { git = "https://github.com/BotResources/br-rust-common", package = "br-scope-declaration-contract", tag = "v1.0.2" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
