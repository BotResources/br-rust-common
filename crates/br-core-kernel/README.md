# br-core-kernel

Typed ID wrappers around `uuid::Uuid` so a user ID can't be confused with a
service-account ID at compile time.

**Purpose.** Every BotResources service refers to identities. Without typed
wrappers, two `Uuid`-typed arguments are interchangeable and a swap is
silent. This crate fixes that with thin newtypes around `Uuid` that keep the
ergonomics that don't reopen the hole (`Display`, `From<Uuid>`,
`Serialize`/`Deserialize`, explicit `as_uuid()` / `AsRef<Uuid>`). They
deliberately do **not** implement `Deref<Target = Uuid>`: deref coercion would
silently turn a `UserId` back into a `&Uuid` wherever a `&Uuid` is expected
(UUID-keyed maps, SQL binds, `&Uuid`-taking functions), defeating the whole
point. Reaching the inner value is always an explicit call.

**When to use.** You need `UserId` or `ServiceAccountId` to pass or store an
actor identifier and want compile-time separation from other UUIDs.

**When not to use.** You need a generic UUID (e.g. a correlation ID) — use
`uuid::Uuid` directly. Don't add project-specific ID types here; keep them
local to their bounded context.

## What's inside

| Item | Kind | Notes |
|---|---|---|
| `UserId(pub Uuid)` | struct | Human user identifier. |
| `ServiceAccountId(pub Uuid)` | struct | Machine identity (service account). |

Both types implement:

- `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`
- `Serialize`, `Deserialize` — `#[serde(transparent)]`, so the JSON wire
  format is a plain UUID string (`"…"`). Serde's default newtype encoding
  already produced this shape; the attribute turns it from an accident of the
  default into an enforced, tested contract that cannot drift silently.
- `Display` (delegates to `Uuid`)
- `From<Uuid>` and `From<UserId> for Uuid` / `From<ServiceAccountId> for Uuid`
  (no checked conversion — the wrap and unwrap are intentionally cheap)
- `as_uuid(&self) -> Uuid` (`const`) and `AsRef<Uuid>` — the explicit, only
  ways to reach the inner UUID. No `Deref`: coercion to `&Uuid` is exactly the
  silent confusion this crate prevents.

## Usage

```rust
use br_core_kernel::{ServiceAccountId, UserId};
use uuid::Uuid;

fn process_request(actor: UserId) {
    // `actor` cannot accidentally be passed where ServiceAccountId is expected.
    println!("user {actor}");
    // Reaching the raw UUID is explicit — by value or by reference.
    let raw: Uuid = actor.as_uuid();
    let raw_ref: &Uuid = actor.as_ref();
    let _ = (raw, raw_ref);
}

let raw: Uuid = Uuid::new_v4();
process_request(raw.into());

// Direct serde — the wire format is a plain UUID string, tested + contractual.
let json = serde_json::to_string(&UserId::from(raw)).unwrap();
```

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-kernel = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-kernel", tag = "br-core-kernel-v0.4.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
