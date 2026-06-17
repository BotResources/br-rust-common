# br-util-nats-fabric

The **Project NATS Fabric API** — the single, restricted, typed application-facing
way a BR service touches NATS. It owns all `async_nats` coupling and exposes a
small surface (`Fabric`) over two concerns: **integration messaging** (commands
and events on fixed streams under a fixed grammar) and the **Published-Language
KV** (generic publisher + consumer mechanics over one fixed bucket).

Tier `util`: it may depend on `core` (`br-core-integration`, `br-core-events`),
never the reverse. It builds on the integration envelopes and the pure outbox
state machine from `br-core-integration`; it does **not** restate them.

## The no-provisioning guarantee

The fabric **never** creates infrastructure. There is no `create_stream`,
`ensure_stream`, `create_bucket`, or `ensure_bucket` anywhere in this crate. A
`Fabric` is constructed *from* an already-connected
`async_nats::jetstream::Context` — it never connects. Streams, durable
consumers, and the KV bucket are **declared out of band** and assumed to exist;
every entry point **binds** an existing object and **fails loud** (a
`FabricError`) when it is absent. Readiness gates this, not runtime auto-repair.

## What the caller may provide — and what it may never provide

The caller supplies **only business coordinates**: the receiver/producer
bounded context, the aggregate, the verb/past-fact, the version, a durable name,
and a typed payload. The caller **never** supplies a stream name, the
`integration` prefix, the grammar, or a standard bucket name — those are frozen
constants the fabric owns. There is **no public freestyle string subject
builder**: subjects are rendered, internally, from validated coordinates only.

## Surface 1 — integration messaging

### The v1 grammar (fixed, 6 segments)

| Kind    | Subject                                              |
| ------- | ---------------------------------------------------- |
| command | `integration.cmd.{receiver}.{aggregate}.{verb}.v{N}` |
| event   | `integration.evt.{producer}.{aggregate}.{fact}.v{N}` |

The `integration` prefix and the `cmd`/`evt` tokens are not caller-choosable.
Each segment is a validated newtype — `Bc`, `Aggregate`, `Verb`, `PastFact` —
that accepts ASCII alphanumerics plus `-` and `_`, is non-empty, and rejects
`.`, the NATS wildcards `*`/`>`, and whitespace. Coordinates are assembled into
`CommandCoords { receiver, aggregate, verb, version }` and
`EventCoords { producer, aggregate, fact, version }`.

These coordinate types are **transport-independent contract types owned by
`br-core-integration`** (so a core contract crate can build on them without a
core→util dependency); this crate re-exports them and owns only the
`integration.…` rendering. `command_subject(&CommandCoords)` /
`event_subject(&EventCoords)` render a coordinate to its wire subject (for
comparison/logging); there is no freestyle string subject builder.

### Fixed streams

| Constant          | Stream name        | Binds                |
| ----------------- | ------------------ | -------------------- |
| `INTEGRATION_CMD` | `INTEGRATION_CMD`  | `integration.cmd.>`  |
| `INTEGRATION_EVT` | `INTEGRATION_EVT`  | `integration.evt.>`  |

### Publishing

```rust,ignore
fabric.publish_command(&coords, &command).await?;
fabric.publish_event(&coords, &event).await?;
// fire-and-forget (best-effort, warns and drops on failure):
fabric.publish_command_if_connected(&coords, &command).await;
fabric.publish_event_if_connected(&coords, &event).await;
```

The envelopes are `br_core_integration::IntegrationCommand<T>` /
`IntegrationEvent<T>`, re-exported here.

### Consuming

`run_commands` / `run_events` bind a **caller-named durable** on the fixed
stream with a **coordinate filter** (the fabric computes the filter subject; the
caller never passes a stream name). The bind **verifies the durable's configured
filter is exactly the expected coordinate subject** — a durable that has been
misconfigured to widen its delivery (e.g. left on `integration.evt.>`) is
rejected with `FabricError::FilterMismatch`, so a consumer can never silently
receive more than its declared coordinates. `verify_command_durable` /
`verify_event_durable` perform the same bind-and-verify without running, for a
readiness gate.

The handler returns a `br_core_integration::MessageOutcome`
(`Ack` / `Nak` / `Term`); a payload that fails to decode is `Term`-ed and routed
to the caller's poison handler — it is never silently dropped.

### Correlated awaiter

`Fabric::await_event(&coords)` opens a subscription scoped to one `EventCoords`
on the fixed event stream; `Fabric::await_events(&[&EventCoords])` awaits **one
of several** reply facts (e.g. a request/reply that resolves on either an
`accepted` or a `rejected` event). `await_correlation(correlation_id, deadline)`
returns the first matching envelope or `None` at the deadline. The caller passes
coordinates, never a stream or filter string.

### Outbox (feature `outbox`)

A transactional outbox whose record destination is a **typed `EventCoords`**, not
a raw subject string. `stage` persists the record and fires the `pg_notify`
wake-up inside the caller's transaction (binding the fixed `integration_outbox`
table). The `OutboxRelay` drains pending rows, **renders the subject from the
typed destination at publish time**, and applies the pure retry/transition state
machine from `br-core-integration`; `RelayHealth` degrades on a structural
(no-stream) failure. The table is assumed to exist — the relay never creates it.

The legacy `br_core_integration::OutboxRecord` (raw `subject: String`) was
removed in the v1.0.0 integration-reduction step; `br_util_nats_fabric::OutboxRecord`
(typed `EventCoords` destination) is now the only outbox record type.

## Surface 2 — Published Language over KV

`PublishedLanguagePublisher::open(&fabric)`,
`PublishedLanguageConsumer::open(&fabric, prefixes, copy_filter, sink)` and
`PublishedLanguageReader::open(&fabric)` are the
only ways in. Each binds the fixed bucket `PUBLISHED_LANGUAGE` **internally** and
fails loud if it is absent. The raw `async_nats` KV `Store` is never handed to a
caller — there is no untyped `store.put(key, …)` / `store.get(key)` escape
hatch; every write and read goes through a validated `KvKey` / `KvPrefix`. This
crate ships **generic mechanics only**; the *policy* — which prefixes, which
entries to copy, what to persist — is a set of **caller-owned seams**.

> `br-util-directory` will be re-expressed on top of this crate's
> Published-Language KV mechanics in the same v1.0.0 train; its own
> `reconcile_entries` / `KvOp` reconcile engine is destined to disappear. The
> `PublishedLanguagePublisher` / `PublishedLanguageConsumer` + reconcile here are
> the canonical generic mechanism.

### Keys

`KvKey` / `KvPrefix` accept `[A-Za-z0-9_./-]`, reject empty and wildcard-like
input (`*`, `>`). Encode/decode is **fail-closed**: a decode failure is an
explicit `FabricError::Decode` naming the key, never a silent skip.

### Publisher mechanics

`put` / `update` are a **semantic upsert** (never delete-then-create for an
object that still belongs); `retract` deletes only for a real disappearance.
`reconcile(prefix, desired)` reads the observed entries under a prefix and
applies the minimal op set (put changed/new, delete orphans); `repair_drift` is
the periodic re-run of the same reconcile.

### Consumer mechanics (the generic enablers of the directory's filter/extension/selection)

`PublishedLanguageConsumer` is generic over the value `V` and parameterised by
three **caller-owned seams**:

- **consumer-selected prefixes** — the consumer chooses which prefixes to scan
  and watch, independent of the publisher (e.g. users-only vs users + groups);
- **a copy-filter `Fn(&V) -> bool`** — decides which entries are projected at
  all; an entry that flips pass → fail is **orphan-deleted (retracted locally)**
  on the next reconcile and on the watch update that carries the failing value;
- **a projection sink (`ProjectionSink<V>`)** — the mechanic **never force-drops
  fields**: the sink receives the **full decoded `V`** and decides exactly what
  to persist (in its own transaction where applicable), so a consumer can
  preserve any extension it wants. Local orphan cleanup is driven by the sink's
  own `known_keys`.

`bootstrap()` does the initial scan-and-project + orphan cleanup; `watch()`
processes live updates from the selected prefixes. `WatchHealth` exposes a
degraded signal when the watch ends or errors. This crate **does not** ship a
transformation DSL — filtering and mapping are the caller's.

### Single-key read

`PublishedLanguageReader::<V>::open(&fabric).get(&key)` reads exactly one entry
by its validated `KvKey` — for the consumer that needs one known key (e.g. the
directory manifest `identity/_meta`) rather than a prefix scan. Semantics:

- **exact-key, not prefix** — only the entry at `key` is returned; a sibling key
  sharing a prefix (`identity/_metadata`) is never matched;
- **fail-closed decode** — an undecodable value is an explicit
  `FabricError::Decode` naming the key, **never** a silent `None`;
- **`Ok(None)` only for a genuinely absent key**;
- **bind-existing** — the fixed `PUBLISHED_LANGUAGE` bucket is bound internally,
  failing loud if absent; no provisioning.

## Generic mechanics vs caller seams (summary)

| Generic (this crate owns)                              | Caller seam                                  |
| ------------------------------------------------------ | -------------------------------------------- |
| the v1 grammar, fixed streams, fixed bucket            | the business coordinates + payload type      |
| subject rendering, durable filter verification         | the durable name                             |
| reconcile op computation, orphan detection             | the desired set                              |
| bootstrap scan + watch loop, fail-closed codec         | the prefix selection                         |
| exact-key single-key read (`PublishedLanguageReader`)  | the `KvKey` to read                          |
| the copy-filter *mechanism*                            | the `Fn(&V) -> bool` predicate               |
| the projection *mechanism* (full `V` to the sink)      | the `ProjectionSink<V>` (what to persist)    |

## Dependency

```toml
br-util-nats-fabric = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-nats-fabric", tag = "v1.0.0", version = "1.0.0" }
# with the transactional outbox:
# br-util-nats-fabric = { git = "...", package = "br-util-nats-fabric", tag = "v1.0.0", version = "1.0.0", features = ["outbox"] }
```
