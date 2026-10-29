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
`integration.cmd.identity.service_scope.declare.v2`,
`integration.evt.identity.service_scope.accepted.v1`, and `…rejected.v1`.

There are two declare commands and one pair of replies. `declare.v1` carries
`br-core-scope`'s `DeclareServiceScopes`; `declare.v2` carries
`DeclareServiceScopesV2`, which adds per-scope `implies` and
`service_accounts_only`. Both are answered on the same `accepted.v1` /
`rejected.v1` facts; a v2 rejection may carry the v2 reasons
(`unknown_implied_scope`, `implied_scope_prefix_mismatch`,
`implication_cycle`). The v1 command keeps working unchanged.

Domain-light by construction: it depends only on `br-core-integration` (for the
coordinate types), holds no aggregate, no event payloads, no policy.

## What's inside

| Item | Kind | Value |
|---|---|---|
| `BC` | `&str` | `"identity"` |
| `AGGREGATE` | `&str` | `"service_scope"` |
| `VERSION` | `u8` | `1` — the v1 declare command and the `accepted` / `rejected` facts |
| `VERSION_V2` | `u8` | `2` — the v2 declare command (scope implications, service-account-only scopes) |
| `COMMAND_NAME` | `&str` | `"declare"` |
| `ACCEPTED` / `REJECTED` | `&str` | confirmation fact names |
| `UNREPRESENTABLE_SERVICE` | `&str` | `"unrepresentable_service"` — service-key sentinel emitted on the `rejected` fact when the declared manifest key is unparsable |
| `declare_command_coords()` | `fn() -> Result<CommandCoords, CoordError>` | receiver=`identity`, aggregate=`service_scope`, verb=`declare`, v1 |
| `declare_v2_command_coords()` | `fn() -> Result<CommandCoords, CoordError>` | the same coordinates at v2 |
| `accepted_event_coords()` / `rejected_event_coords()` | `fn() -> Result<EventCoords, CoordError>` | producer=`identity`, aggregate=`service_scope`, fact=`accepted`/`rejected`, v1 |
| `command_type()` | `fn() -> String` | `service_scope.declare` |
| `event_type(name)` | `fn(&str) -> String` | `service_scope.<name>` |

The canonical coordinate assertion lives in this crate's tests; the Fabric owns
the wire-string assertion; the consuming crates assert only their own
composition logic.

## Install

`br-scope-declaration-contract/v0.1.1` pins `br-core-integration/v0.1.0` and returns its `CommandCoords`, `EventCoords` and `CoordError`. Take `br-core-integration` (and `br-util-nats-fabric`, which hands the same coordinates to the broker) at their `0.1.0` tags in the same service: another tag (such as `v1.4.0`) holds a second copy of the type, and passing a coordinate across fails to compile (E0308).

```toml
[dependencies]
br-scope-declaration-contract = { git = "https://github.com/BotResources/br-rust-common", package = "br-scope-declaration-contract", tag = "br-scope-declaration-contract/v0.1.1", version = "0.1.1" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](./CHANGELOG.md) · [botresources.ai](https://botresources.ai)
