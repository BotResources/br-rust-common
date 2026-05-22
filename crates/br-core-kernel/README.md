# br-core-kernel

Typed ID wrappers around `uuid::Uuid` so a user ID can't be confused with a
service-account ID at compile time.

**Purpose.** Every BotResources service refers to identities. Without typed
wrappers, two `Uuid`-typed arguments are interchangeable and a swap is
silent. This crate fixes that with thin `repr(Uuid)` newtypes that keep
ergonomics (`Display`, `Deref`, `From<Uuid>`, `Serialize`/`Deserialize`)
intact.

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
- `Serialize`, `Deserialize` (transparent — JSON wire format = a plain UUID string)
- `Display` (delegates to `Uuid`)
- `From<Uuid>` (no checked conversion — the wrap is intentionally cheap)
- `Deref<Target = Uuid>` (so `.to_string()`, `.as_bytes()`, etc. work directly)

## Usage

```rust
use br_core_kernel::{ServiceAccountId, UserId};
use uuid::Uuid;

fn process_request(actor: UserId) {
    // `actor` cannot accidentally be passed where ServiceAccountId is expected.
    println!("user {actor}");
}

let raw: Uuid = Uuid::new_v4();
process_request(raw.into());

// Direct serde — wire format is a plain UUID string, not `{"0": "..."}`.
let json = serde_json::to_string(&UserId::from(raw)).unwrap();
```

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-kernel = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-kernel", tag = "br-core-kernel-v0.3.1" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
