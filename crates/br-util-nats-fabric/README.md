# br-util-nats-fabric

The **Project NATS Fabric API** — the single, restricted, typed application-facing
way a BR service touches NATS. It owns all `async_nats` coupling and exposes a
small surface (`Fabric`) over three concerns: **integration messaging** (commands
and events on fixed streams under a fixed grammar), the **Published-Language KV**
(generic publisher + consumer mechanics over the fixed `PUBLISHED_LANGUAGE`
bucket), and the **Ephemeral-Auth KV** (compare-and-swap mechanics over the fixed
`EPHEMERAL_AUTH` bucket).

Tier `util`: it may depend on `core` (`br-core-integration`, `br-core-events`),
never the reverse. It builds on the integration envelopes and the pure outbox
state machine from `br-core-integration`; it does **not** restate them.

## The no-provisioning guarantee

The fabric **never** creates infrastructure. There is no `create_stream`,
`ensure_stream`, `create_bucket`, or `ensure_bucket` anywhere in this crate.
Connecting to the broker is **not** provisioning: the fabric dials an existing
NATS server (`Fabric::connect`/`connect_with`) but creates no JetStream object.
Streams, durable consumers, and the two KV buckets (`PUBLISHED_LANGUAGE`,
`EPHEMERAL_AUTH`, including the latter's TTL `max_age`) are **declared out of
band** and assumed to exist; every entry point **binds** an existing object and
**fails loud** (a `FabricError`) when it is absent. Readiness gates this, not
runtime auto-repair.

## Constructing a `Fabric`

The boot-time dial is confined to this crate — a service never reaches for
`async_nats` directly:

```rust,ignore
let fabric = Fabric::connect("nats://nats:4222").await?;
let fabric = Fabric::connect_with(
    "nats://nats:4222",
    &NatsAuth { user, password },
).await?;
```

`connect` dials anonymously; `connect_with` dials with a user/password
(`NatsAuth { user, password }` — a typed pair that keeps `async_nats` out of the
public signature). `NatsAuth` carries a hand-written `Debug` that masks the
password (`password: "***"`) so the credential can never leak through a
debug-print or a structured log. Both build the JetStream context internally and
return a ready `Fabric`. A failed dial surfaces as the distinct, matchable
`FabricError::Connect`. In-cluster transport is plaintext per the trust model, so
there is no TLS/credentials-file surface. `Fabric::new(jetstream::Context)`
remains for tests and advanced callers that already own a context.

### Reachability probe

`Fabric::reachable() -> bool` and `Fabric::connection_state() -> ConnectionState`
expose the client's **locally-cached** connection view for a readiness/liveness
gate. `ConnectionState` is the fabric's own enum (`Pending` / `Connected` /
`Disconnected`) — the raw `async_nats` `State` is never exposed across the public
API. Be honest about what this is: it is the **cached** view `async_nats`
maintains from its connection loop, **not** a guaranteed live round-trip — a probe
in the millisecond after a silent disconnect can still read `Connected` until the
client's own ping/health detects it. For a **true round-trip**, `Fabric::ping()` flushes the
client to the server and surfaces a `FabricError::Connect` if the broker does not
answer — distinctly named so a caller never mistakes the cheap cached view for the
round-trip. The fail-loud startup check remains `connect` (the real dial): an
unreachable broker at boot fails `connect` and readiness stays DOWN.

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
// idempotent (sets the Nats-Msg-Id dedup header; caller owns the id):
fabric.publish_command_with_id(&coords, &command, &message_id).await?;
fabric.publish_event_with_id(&coords, &event, &message_id).await?;
// fire-and-forget (best-effort, warns and drops on failure):
fabric.publish_command_if_connected(&coords, &command).await;
fabric.publish_event_if_connected(&coords, &event).await;
```

The envelopes are `br_core_integration::IntegrationCommand<T>` /
`IntegrationEvent<T>`, re-exported here.

#### Idempotent publish (dedup id)

`publish_command_with_id` / `publish_event_with_id` are the plain `publish_*`
variants that additionally set the JetStream `Nats-Msg-Id` header from a
caller-supplied id (typically the domain event's UUIDv7). Two publishes that
carry the same id within the stream's configured duplicate window are deduped by
the broker to a single stored message, so a retry after an ambiguous ack does not
double-write. These variants are for callers managing their **own** idempotency;
the **sanctioned reliable / exactly-once-ish path is the `outbox` feature** — its
relay owns the staging, retry and at-least-once delivery, and a dedup id on the
published frame collapses the at-least-once into effectively-once on the
consumer's stream. The caller owns the id; the fabric never mints one.

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

#### The durable consumer (explicit per-delivery acknowledgement)

For the production work loop that needs to inspect the redelivery count and own
the ack decision per delivery (a poison budget, a transactional side effect
before ack), bind a typed consumer and pull deliveries explicitly. The durable's
own `max_deliver` is the authoritative budget; the app-side `term()` below is an
**optional second barrier** the caller may add when it wants to stop a frame
before the broker's `max_deliver` is reached (e.g. a tighter per-handler ceiling):

```rust,ignore
const APP_MAX_DELIVER: i64 = 5;
const NAK_DELAY: Duration = Duration::from_secs(2);

let mut consumer = fabric.bind_command_consumer::<T>(&coords, "svc-notifier").await?;
while let Some(delivery) = consumer.recv().await? {
    match (delivery.delivered_count(), delivery.payload()) {
        // Optional second barrier: stop before the durable's own max_deliver.
        (Some(count), _) if count > APP_MAX_DELIVER => delivery.term().await?,
        (Some(_), Ok(command)) => {
            if do_work(command).await.is_ok() {
                delivery.ack().await?;
            } else {
                delivery.nak(Some(NAK_DELAY)).await?;
            }
        }
        // Fail-closed: a poison frame, or one whose delivery info is absent
        // (so its budget cannot be tracked), is routed to term.
        (_, Err(_unprocessable)) | (None, _) => delivery.term().await?,
    }
}
```

- `bind_command_consumer::<T>(&CommandCoords, durable)` /
  `bind_event_consumer::<T>(&EventCoords, durable)` **bind an existing durable**
  and **fail loud** — the same coordinate-filter verification as
  `run_commands`/`run_events` (a widened durable is rejected with
  `FilterMismatch`), and a `FabricError::Consume` (`NoConsumer` / `NoStream`)
  when the durable or stream is absent. The fabric never creates the consumer.
- `bind_event_consumer_many::<T>(&[&EventCoords], durable)` binds one durable
  that **fans in several event coordinates** — the svc-pm-style consumer that
  reads `user.created` + `user.updated` + `group.created` on a single durable.
  `T` is the caller's union type, deserialized per frame and **fail-closed**
  exactly as the single-coordinate path. The bind verifies the durable's
  configured `filter_subjects` equal the rendered set **exactly, order-insensitive
  (set equality)** — a durable that filters more, fewer, or different subjects
  (including the wildcard `integration.evt.>`) is rejected with `FilterMismatch`,
  so a fan-in consumer still cannot silently widen beyond its declared
  coordinates. `bind_event_consumer` is the 1-coordinate case of this. There is
  **no command-side fan-in**: a command durable is receiver-owned, one
  `aggregate.verb` per durable. The wildcard subscription stays **rejected** —
  generic/wildcard delivery is a gitops concern, not a fabric one.
- `recv()` yields the next `Delivered<E>` (`None` once the stream ends; a
  matchable transport `FabricError::Consume` on a broker/consumer-gone error).
- `Delivered<E>` exposes `payload() -> Result<&E, &FabricError>` — a malformed
  wire frame is **fail-closed**: it surfaces as a `FabricError::Decode` naming
  the subject that the caller routes to `term()`, **never** a silent drop and
  **never** a panic that ends the loop.
- `delivered_count() -> Option<i64>` is the JetStream delivery attempt count.
  It is `None` when the frame's delivery info is **absent** — the count that
  drives the poison budget cannot be fabricated, so the absence is **observable**
  and the frame is independently routable to `term()`
  (`payload()` is then a `FabricError::Consume { kind: NoDeliveryInfo }`),
  never a silent `1` that would let a poison frame evade the budget forever.
- `ack()`, `nak(Option<Duration>)`, `term()` are the three typed ack outcomes.
  An ack-path transport failure is classified: `ConsumerGone` when the
  consumer/responders are gone, `Other` otherwise.
- No raw `async_nats` `Message` / `Consumer` / `Context` / `AckKind` is exposed.
  `CommandConsumer<T>` / `EventConsumer<T>` alias
  `IntegrationConsumer<IntegrationCommand<T>>` /
  `IntegrationConsumer<IntegrationEvent<T>>`.

#### Graceful shutdown (SIGTERM-safe)

`recv()` is **cancel-safe**: it may be dropped at any `.await` point inside a
`tokio::select!` without losing a message — a frame is only consumed once it has
been yielded as a `Delivered<E>`, and the per-delivery `ack()` / `nak()` /
`term()` lives on that owned `Delivered<E>`, not inside `recv()`. So the
SIGTERM-safe shape is to race `recv()` against the shutdown signal, finish the
**in-flight** frame's ack on the branch that already holds a `Delivered<E>`, then
stop pulling:

```rust,ignore
loop {
    tokio::select! {
        biased;
        _ = shutdown.recv() => break,
        next = consumer.recv() => match next? {
            Some(delivery) => { /* do_work + delivery.ack()/nak()/term() */ }
            None => break,
        },
    }
}
consumer.drain().await;
```

`drain(self)` **consumes** the consumer and closes the underlying subscription
cleanly (the pull task is aborted and the inbox unsubscribed on drop) — it stops
pulling without panicking and without losing a message: a frame whose `ack()`
already completed is not redelivered, and a frame still un-acked at drain is left
**un-acked** and is redelivered after `ack_wait` (at-least-once is preserved, no
silent drop). The contract is: **finish the in-flight ack on the held
`Delivered<E>` first, then `drain()`.**

### Correlated awaiter

`Fabric::await_event(&coords)` opens a subscription scoped to one `EventCoords`
on the fixed event stream; `Fabric::await_events(&[&EventCoords])` awaits **one
of several** reply facts (e.g. a request/reply that resolves on either an
`accepted` or a `rejected` event). The symmetric command-side surface,
`Fabric::await_command(&coords)` / `Fabric::await_commands(&[&CommandCoords])`,
binds the fixed command stream instead — for observing a command in flight (e.g.
a `declare` a service is about to consume). Both fail loud if the bound stream is
absent and never auto-create it. `await_correlation(correlation_id, deadline)`
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
processes live updates from the selected prefixes. `watch()` subscribes to the
**whole bucket** and filters each entry by the selected prefixes client-side
(`KvPrefix::matches`) — **not** a per-prefix subject wildcard: NATS subject
wildcards match only across `.`-delimited tokens, and the Published-Language keys
are `/`-delimited (`identity/users/<id>` is a single token), so a `{prefix}>`
filter matches nothing and would silently deliver no live updates. `WatchHealth`
exposes a degraded signal when the watch ends or errors. This crate **does not**
ship a transformation DSL — filtering and mapping are the caller's.

### Single-key read

`PublishedLanguageReader::<V>::open(&fabric).get(&key)` reads exactly one entry
by its validated `KvKey` — for the consumer that needs one known key (e.g. the
directory manifest `identity/_meta`) rather than a prefix scan. Semantics:

- **exact-key, not prefix** — only the entry at `key` is returned; a sibling key
  sharing a prefix (`identity/_metadata`) is never matched;
- **fail-closed decode** — an undecodable value is an explicit
  `FabricError::Decode` naming the key, **never** a silent `None`;
- **store-access failure surfaces** — a broker/KV outage during the read is an
  explicit `FabricError::Kv`, never collapsed to `Ok(None)`;
- **`Ok(None)` only for a genuinely absent key**;
- **bind-existing** — the fixed `PUBLISHED_LANGUAGE` bucket is bound internally,
  failing loud if absent; no provisioning.

### Enumeration

`PublishedLanguageReader::<V>::keys(&prefix)` and `entries(&prefix)` are the typed
prefix scan — for the consumer that must project **all** entries under a prefix
(e.g. the directory projecting every user/group during its bootstrap/reconcile),
without dropping to a raw `async_nats` `Store` key-scan. Semantics:

- **prefix-scoped** — only keys under `prefix` (by `KvPrefix::matches`) are
  returned; an entry outside the prefix is never included;
- `keys` returns the validated `KvKey`s (sorted); `entries` returns a
  `BTreeMap<KvKey, V>` of the decoded values;
- **decode contract** — `keys()` enumerates keys **without decoding values** (it
  cannot fail-closed on a value), while `entries()` materializes the values and
  **fail-closes** with a `FabricError::Decode` naming the undecodable key;
- **fail-closed decode** — `entries` surfaces an undecodable value as an explicit
  `FabricError::Decode` naming the key, **never** a silent skip;
- **store-access failure surfaces** — a broker/KV outage during the scan is an
  explicit `FabricError::Kv`, never collapsed to an empty result;
- **bind-existing** — the fixed `PUBLISHED_LANGUAGE` bucket is bound internally,
  failing loud if absent; no provisioning.

## Surface 3 — Ephemeral Auth over KV (compare-and-swap)

`EphemeralAuthStore::<V>::open(&fabric)` is the only way in. It binds the fixed
bucket `EPHEMERAL_AUTH` **internally** and fails loud if it is absent; the
bucket's TTL (`max_age`) is declared at provisioning and the opener only binds,
never provisions. As with the Published-Language facades, the raw `async_nats`
KV `Store` is never handed to a caller — there is no untyped escape hatch, and
the compare-and-swap contract below is the **only** sanctioned revision-aware
path.

This surface exists for credential state that needs **optimistic concurrency**
— the canonical consumer is svc-auth refresh-token rotation, whose
family-reuse-detection requires that two concurrent rotations on the same family
cannot both win (a last-write-wins clobber would break the revision chain and
blind reuse-detection).

- `get_with_revision(&KvKey) -> Result<Option<(V, Revision)>, FabricError>` reads
  the current value and its `Revision`. A genuinely absent key (or a deleted /
  purged tombstone) is `Ok(None)`; an undecodable value is **fail-closed**
  (`FabricError::Decode` naming the key), never a silent `None`; a broker/KV
  outage surfaces as `FabricError::Kv`.
- `create(&KvKey, &V) -> Result<(), FabricError>` is the **create path** and the
  only correct way to occupy a key the caller believes is free. It succeeds when
  the key has never lived **and** when it previously lived then expired (TTL
  `max_age`) or was deleted — both leave a KV tombstone at a sequence `> 0`, and
  `create` re-creates against that tombstone, which is the nominal refresh-family
  lifecycle. A key that is currently **live** is a distinguishable, matchable
  `FabricError::KeyAlreadyExists { key }`. **Use `create` for family creation /
  re-creation** — it is the broker-correct way to occupy a key whether it never
  lived or previously lived then expired/was deleted (both leave a tombstone at a
  sequence `> 0`).
- `update_if(&KvKey, &V, Revision) -> Result<(), FabricError>` is the
  **rotate path**: a revision-checked write that succeeds only if the supplied
  `Revision` is still the last revision for the key (read it from
  `get_with_revision`, write it back here). On a revision mismatch it returns the
  first-class, matchable `FabricError::RevisionConflict { key, expected }` —
  distinct from not-found (`Ok(None)` on read), `KeyAlreadyExists`, transport
  (`Kv`) and `Decode`, so the caller can drive reuse-detection on it.
- `delete_if(&KvKey, Revision) -> Result<(), FabricError>` is the
  **revision-checked delete**: it writes a delete tombstone (so a subsequent
  `get_with_revision` reads `Ok(None)`) only if the supplied `Revision` is still
  the last revision for the key. On a revision mismatch it returns the same
  first-class, matchable `FabricError::RevisionConflict { key, expected }` as
  `update_if`, and the key is left untouched — the canonical use is
  logout-vs-rotation, where an explicit session invalidation must not clobber a
  concurrent rotation.
- `delete(&KvKey) -> Result<(), FabricError>` is the **unconditional** delete,
  ignoring the revision chain — it writes a delete tombstone regardless of
  concurrent rotations, the delete counterpart of `put`.
- `put(&KvKey, &V)` is the **unconditional** write, ignoring the revision chain —
  for the `revoke_family` wipe that must land regardless of concurrent rotations.
- `status()` exposes the **bound bucket's cached KV state** in the bind-existing
  posture — it reads `async_nats`'s locally-cached stream info and does **not**
  round-trip the broker, so it is **not** a live reachability probe and must not
  back a liveness gate. The fail-loud liveness check is `open()` (the real bind
  round-trip): if the bucket is unreachable at startup, `open()` fails and
  readiness stays DOWN.

`Revision` is an opaque newtype over the NATS KV sequence — the caller reads it
from `get_with_revision` and passes it back to `update_if` or `delete_if`. A
caller never mints a `Revision` by hand; every revision originates from
`get_with_revision`.

## Generic mechanics vs caller seams (summary)

| Generic (this crate owns)                              | Caller seam                                  |
| ------------------------------------------------------ | -------------------------------------------- |
| the v1 grammar, fixed streams, fixed bucket            | the business coordinates + payload type      |
| subject rendering, durable filter verification         | the durable name                             |
| reconcile op computation, orphan detection             | the desired set                              |
| bootstrap scan + watch loop, fail-closed codec         | the prefix selection                         |
| exact-key single-key read (`PublishedLanguageReader`)  | the `KvKey` to read                          |
| prefix enumeration (`PublishedLanguageReader::keys`/`entries`) | the `KvPrefix` to scan                 |
| compare-and-swap KV (`EphemeralAuthStore`, `Revision`) | the `KvKey`, the value, the observed revision |
| the copy-filter *mechanism*                            | the `Fn(&V) -> bool` predicate               |
| the projection *mechanism* (full `V` to the sink)      | the `ProjectionSink<V>` (what to persist)    |

## Why

| Thing | Why it is the way it is |
| ----- | ----------------------- |
| `IntegrationConsumer::drain()` is `async` though it currently only drops the pull stream | The signature reserves a future awaiting drain (in-flight-ack / unsubscribe flush) and avoids a later breaking sync→async change. |

## Dependency

```toml
br-util-nats-fabric = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-nats-fabric", tag = "v1.0.2", version = "1.0.2" }
# with the transactional outbox:
# br-util-nats-fabric = { git = "...", package = "br-util-nats-fabric", tag = "v1.0.2", version = "1.0.2", features = ["outbox"] }
```
