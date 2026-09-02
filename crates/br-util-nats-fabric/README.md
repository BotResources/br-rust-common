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

## The provisioning boundary

The fabric **never** creates **infra**. There is no `create_stream`,
`ensure_stream`, `create_bucket`, or `ensure_bucket` anywhere in this crate.
Connecting to the broker is **not** provisioning: the fabric dials an existing
NATS server (`Fabric::connect`/`connect_with`) but creates no JetStream object.
**Streams and the two KV buckets** (`PUBLISHED_LANGUAGE`, `EPHEMERAL_AUTH`,
including the latter's TTL `max_age`) are **declared out of band by gitops** and
assumed to exist; every entry point **binds** an existing one and **fails loud**
(a `FabricError`) when it is absent. Readiness gates this, not runtime
auto-repair.

The boundary falls at the **durable consumer**: a durable carries the *service's*
processing semantics (ack policy, `ack_wait`, `max_ack_pending`, `max_deliver`,
deliver/replay) — not infra — and is cheap + idempotent to (re)create, so the
fabric **does** create its durable consumers (via `create_consumer`, create-or-
update, so replicas share and a pre-existing durable converges to the fabric's
config). The fabric owns the `ConsumerConfig`; the caller passes only typed coords
plus a durable name, never a raw config. So the rule is: **streams plus buckets
are gitops-declared, bind-only, fail-loud; durable consumers are fabric-created.**

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
// idempotent, plus the broker's verdict on this frame:
let outcome = fabric.publish_command_with_id_outcome(&coords, &command, &message_id).await?;
let outcome = fabric.publish_event_with_id_outcome(&coords, &event, &message_id).await?;
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
published frame collapses the at-least-once into effectively-once **within the
stream's duplicate window**, and only within it. The caller owns the id; the
fabric never mints one.

`publish_command_with_id_outcome` / `publish_event_with_id_outcome` are the same
publishes, returning the broker's verdict instead of `()`:

```rust,ignore
#[non_exhaustive]
pub enum PublishOutcome {
    Stored { sequence: u64 },
    Duplicate { sequence: u64 },
}
impl PublishOutcome {
    pub fn sequence(&self) -> u64;
    pub fn is_duplicate(&self) -> bool;
}
```

`Duplicate` means the broker recognised the `Nats-Msg-Id` inside the stream's
duplicate window and did **not** store a second copy; `sequence` is the sequence
of the frame it already holds. Both outcomes are a **success** — the frame is on
the stream either way. The plain `_with_id` methods are unchanged, still return
`()`, and now delegate to the `_outcome` ones, so a caller that does not care
about the verdict keeps its current code.

### Consuming

`run_commands` / `run_events` **create-or-bind** a **caller-named durable** on the
fixed stream with a **coordinate filter** (the fabric computes the filter subject
from the typed coords; the caller never passes a stream name nor a raw config).
The durable is the **dimensioning boundary**: a stream and a KV bucket are infra,
declared out of band by gitops and only ever **bound** (fail-loud if absent); a
durable consumer carries the *service's* processing semantics and is cheap +
idempotent to (re)create, so the fabric **creates it** — `stream.create_consumer`
is create-or-update, so two replicas creating the same durable with the same
config share it, and a pre-existing durable converges to the fabric's config (the
fabric's config is authoritative — a durable left widened on `integration.evt.>`
is narrowed to the coordinate filter, never left widened). The stream must still
exist (gitops); an absent stream fails loud with
`FabricError::Consume { kind: NoStream }`. An **empty coordinate set** is rejected
before any create (it would make the consumer vacuum the whole stream) with
`FabricError::FilterMismatch`. `ensure_command_durable` / `ensure_event_durable`
perform the same create-or-bind without running, for a service that wants its
durable to exist and start accumulating before the work loop opens; their
`ensure_command_durable_with` / `ensure_event_durable_with` variants take a
`&ConsumerTuning`, exactly as the consume path does.

The fabric owns the durable's `ConsumerConfig` — the contract is:

| Setting          | Value                          | Why                                                            |
| ---------------- | ------------------------------ | -------------------------------------------------------------- |
| `durable_name`   | the caller's durable           | the only caller input besides the coords                       |
| `filter_subject(s)` | the rendered coordinate set | the fabric derives it; a single coord on `filter_subject`, a fan-in set on `filter_subjects` |
| `ack_policy`     | `Explicit`                     | per-delivery ack, the pull work-loop contract                  |
| `ack_wait`       | 30s (tunable)                  | redelivery grace for a frame in flight — `ConsumerTuning::ack_wait` |
| `max_ack_pending`| 256 (tunable)                  | bounded in-flight pull window, back-pressure — `ConsumerTuning::max_ack_pending` |
| `max_deliver`    | -1 (unlimited)                 | poison handling is the caller's `term()`, not a silent drop-on-budget |
| `deliver_policy` | `All`                          | a fresh durable replays the stream from the start              |
| `replay_policy`  | `Instant`                      | catch-up at full speed, not original pacing                    |

Two settings are caller-tunable through `ConsumerTuning { ack_wait, max_ack_pending }`
— a service with long-running handlers raises `ack_wait` so a frame in flight is
not redelivered while still being processed, and sizes `max_ack_pending` to its
concurrency. `max_ack_pending` follows JetStream wire semantics — use `>= 1` for a
bounded back-pressure window; `0` or a negative value means **unbounded** (no
back-pressure), so set it deliberately. `ConsumerTuning::default()` is the `30s` /
`256` row above. The rest
of the config is not tunable today. `ack_policy = Explicit`,
`deliver_policy = All`, `replay_policy = Instant` and the rendered
`filter_subject(s)` are **frozen as contract** — they *are* the pull work-loop
and the anti-over-delivery guarantee, so changing one breaks the contract rather
than tuning it. `max_deliver = -1` is a different case: it is a **uniform default
today, legitimately per-service tomorrow**. Unlimited redelivery makes poison
handling the caller's explicit `term()` instead of a silent drop-on-budget, which
is the right default, but a service that wants a finite delivery budget and a
dead-letter path has a real need — and the answer is then a **new opt-in seam on
`ConsumerTuning`**, exactly as `ack_wait` and `max_ack_pending` already are.
Never a reach-around: the raw `async_nats` `pull::Config` is not exposed at any
version, the escape hatch stays closed, and a service needing another value files
it as a gap here rather than dropping to raw `async-nats`. Each consumer entry point has a `_with` variant taking `&ConsumerTuning`
(`ensure_command_consumer_with`, `ensure_event_consumer_with`,
`ensure_event_consumer_many_with`, `run_commands_with`, `run_events_with`,
`ensure_command_durable_with`, `ensure_event_durable_with`); the
no-suffix forms delegate with `ConsumerTuning::default()`, so existing callers are
unaffected.

The handler returns a `br_core_integration::MessageOutcome`
(`Ack` / `Nak` / `Term`); a payload that fails to decode is `Term`-ed and routed
to the caller's poison handler — it is never silently dropped.

#### Transient-error recovery (the managed loop survives a missed heartbeat)

`run_commands` / `run_events` do **not** die on a transient stream error. An
error item classified `HeartbeatMissed` (async-nats missed-idle-heartbeat
detection — a transport hiccup; the durable is typically intact server-side),
`NoResponders` (no JetStream API responder — what a pull request gets for the
seconds right after a NATS server restart, before the JS API subjects are back
up) or `Other` (unclassified transport failure) triggers **in-loop recovery**:
the loop sleeps a bounded exponential backoff (200 ms doubling to a 30 s cap),
re-binds the durable through the same create-or-bind path (`ensure_durable`, so
the config converges exactly as at first bind) and resumes, emitting a structured
`warn` per attempt.

`HeartbeatMissed` and `NoResponders` matter as the two classes lifted **out of
the terminal `ConsumerGone` bucket**: they can only ever arrive as the
**initial** trigger from the delivery stream (`source.next()`), and lifting them
out is what lets a transient blip *start* recovery instead of killing the loop on
the first error. `NoResponders` additionally earns its **own diagnostic class**
so a post-restart window is distinguishable in the logs from a genuine
heartbeat gap. The initial trigger is **not** counted against the budget — it
buys one free re-bind attempt. But once recovery is under way the only retried
operation is the re-bind itself, and a re-bind failure classifies as either
`NoStream` (fail-loud terminal — the stream is gitops infra, its absence is never
retried) or `Other` — **never** `HeartbeatMissed`/`NoResponders` again. So a
**prolonged** outage escalates on the `Other` budget:

- `Other` carries a budget of **10 consecutive failures** with no delivered frame
  in between, after which the loop returns the last error — fail loud, for the
  caller's supervisor/readiness to surface — instead of degrading into a silent
  zombie consumer.

In practice async-nats **buffers during its own auto-reconnect**, so the re-bind
typically blocks until the broker is back and succeeds on the **first** attempt;
the `Other` budget is the backstop for an outage that outlasts the reconnect or a
re-create that keeps failing (revoked credentials, an immutable config conflict).
Backoff and budget reset on a **delivered frame**, not on a successful re-bind,
so a flapping broker cannot defeat the escalation. A deleted durable is
re-created by the re-bind (create-or-update) and the loop resumes; a genuinely
deleted consumer surfaces as `ConsumerGone` and terminates. The initial bind
stays fail-loud and is **not** covered by recovery: `run_*` returns the first
bind error to the caller, recovery only guards a loop that already started
running.

#### Readiness verification (`verify_*_durable`) — a probe, not a provision

`verify_command_durable` / `verify_event_durable` answer one narrow boot
question: *does the stream exist, and does it bind this coordinate?* They
**create nothing**. The check is two steps against the fixed stream:

1. `get_stream` on `INTEGRATION_CMD` / `INTEGRATION_EVT` — an absent stream is
   the gitops fail-loud, `FabricError::Consume { kind: NoStream }`.
2. the rendered coordinate subject must be **covered by the stream's configured
   `subjects`**, matched with NATS token semantics (`*` = exactly one token, `>` =
   one-or-more tail tokens, anything else literal). A subject the stream does not
   bind fails with `FabricError::SubjectNotCovered { stream, subject, configured }`
   — the message names the stream, the coordinate, and what the stream actually
   binds.

**What the probe does not check** — read this before gating readiness on it. It
does not exercise the right to create or consume a durable, and it does not
validate the durable name. Up to `1.2.0` these were checked *incidentally*,
because the probe created a real consumer; they no longer are. A NATS user
holding `STREAM.INFO` but lacking consumer-create permission now passes the
probe and fails at the first `run_*` / `ensure_*_consumer`. The probe also says
nothing about the presence of a producer, of messages, or of an existing durable.
It proves stream presence and subject coverage — nothing more. Coverage is read
from the stream's own configured `subjects`, so a stream fed only by a `mirror`
or a `sources` block — which binds no subjects of its own — never covers any
coordinate and fails the probe permanently; the fixed `INTEGRATION_CMD` /
`INTEGRATION_EVT` are declared with explicit subjects, and a mirrored variant is
outside what this probe can verify.

The `durable` argument is retained for signature stability and call-site
readability; it names the readiness gate, and no consumer bearing it is created.
A service that wants its durable to exist and start accumulating at boot — and
that wants the create permission proven — calls `ensure_*_durable` instead; the
two are deliberately different operations.

#### The durable consumer (explicit per-delivery acknowledgement)

For the production work loop that needs to inspect the redelivery count and own
the ack decision per delivery (a poison budget, a transactional side effect
before ack), create-or-bind a typed consumer and pull deliveries explicitly. The
fabric's `max_deliver` default is **unlimited** — poison handling is the caller's
`term()`, never a silent drop-on-budget — so the app-side `term()` below is the
authoritative poison ceiling, applied when the redelivery count crosses a tighter
per-handler budget:

```rust,ignore
const APP_MAX_DELIVER: i64 = 5;
const NAK_DELAY: Duration = Duration::from_secs(2);

let mut consumer = fabric.ensure_command_consumer::<T>(&coords, "svc-notifier").await?;
while let Some(delivery) = consumer.recv().await? {
    match (delivery.delivered_count(), delivery.payload()) {
        (Some(count), _) if count > APP_MAX_DELIVER => delivery.term().await?,
        (Some(_), Ok(command)) => {
            if do_work(command).await.is_ok() {
                delivery.ack().await?;
            } else {
                delivery.nak(Some(NAK_DELAY)).await?;
            }
        }
        (_, Err(_unprocessable)) | (None, _) => delivery.term().await?,
    }
}
```

- `ensure_command_consumer::<T>(&CommandCoords, durable)` /
  `ensure_event_consumer::<T>(&EventCoords, durable)` **create-or-bind the
  durable** with the fabric's authoritative config (the table above), and
  **fail loud only on an absent stream** (`FabricError::Consume`,
  `kind: NoStream` — the stream is gitops). A pre-existing durable converges to
  the fabric's coordinate filter; two replicas calling the same durable share it.
- `ensure_event_consumer_many::<T>(&[&EventCoords], durable)` creates-or-binds one
  durable that **fans in several event coordinates** — the svc-pm-style consumer
  that reads `user.created` + `user.updated` + `group.created` on a single
  durable. `T` is the caller's union type, deserialized per frame and
  **fail-closed** exactly as the single-coordinate path. The fabric sets the
  durable's `filter_subjects` to the rendered set, so the consumer can never
  silently widen beyond its declared coordinates. `ensure_event_consumer` is the
  1-coordinate case of this. An **empty coordinate set** is rejected before any
  create with `FabricError::FilterMismatch` (it would vacuum the whole stream).
  There is **no command-side fan-in**: a command durable is receiver-owned, one
  `aggregate.verb` per durable. The wildcard subscription stays **rejected** —
  generic/wildcard delivery is a gitops concern, not a fabric one.
- `recv()` yields the next `Delivered<E>` (`None` once the stream ends; a
  matchable transport `FabricError::Consume` on a broker/consumer-gone error).
  A missed idle heartbeat surfaces as `Consume { kind: HeartbeatMissed }` and a
  post-restart missing JS responder as `Consume { kind: NoResponders }` — both
  transient, the durable is typically intact: re-bind (`ensure_*_consumer`) and
  resume, do not treat either like the permanent `ConsumerGone`. The explicit
  `recv()` loop does **not** auto-recover — that is the managed `run_*` loop's
  behavior; here the re-bind decision is the caller's.
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
- `progress()` sends the JetStream **working ack** on a `&Delivered<E>`: it
  **resets the server's `ack_wait` timer** without acking or consuming the frame,
  so a handler that may exceed `ack_wait` calls it periodically to avoid being
  redelivered mid-processing. It does not finalise the delivery — a final
  `ack()` / `nak()` / `term()` is still required — and maps its transport failure
  to `FabricError` exactly as `ack` does.
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

#### Dedup id on every relayed frame

The relay publishes each row with a `Nats-Msg-Id` header. The id is the
envelope's `event_id` lifted from the stored payload — the field
`OutboxRecord::stage_event` writes for every `IntegrationEvent<T>` — so a relay
retry, a crash between the publish and the status write, and a rolling deploy
that briefly runs two relays all resolve to the same key and the broker collapses
them to one stored frame.

**JetStream dedup is scoped to the stream, not to the subject.** `INTEGRATION_EVT`
binds `integration.evt.>` for every producer, so the rule is: **one `event_id` =
one frame per stream per window**, whatever the coordinates. Staging the same
`IntegrationEvent` — same `event_id` — to two different `EventCoords` inside the
window publishes the first and makes the second **silently vanish** while its
outbox row still goes `PUBLISHED`; only `RelayPass.duplicates` and the warn line
reveal it. Fanning one fact out to N coordinates therefore needs **N distinct
`event_id`s** — which the "one fact per event" rule already implies: two
coordinates are two facts.

**The rule is positional, not typed: the dedup key is the top-level `event_id`
field of the stored payload, whatever staged it.** The relay reads the persisted
JSON, and if it holds a top-level `event_id` whose value is a UUID string, that
UUID becomes `Nats-Msg-Id`. `OutboxRecord::stage_event` writes that field for
every `IntegrationEvent<T>`, which is the contracted case — but a raw
`OutboxRecord::stage(id, coords, value)` payload is **uncontracted caller
content**, and if it happens to carry a top-level UUID `event_id` it is promoted
exactly the same way. **A raw-stage caller therefore owns that field's
uniqueness.** Two raw rows sharing one `event_id` inside the stream's duplicate
window publish once: the broker answers the second with a duplicate ack, drops
the frame, and the relay — which correctly reads that ack as acceptance — still
marks the second row `PUBLISHED`. The loss is visible only as
`RelayPass.duplicates`, the `"duplicate publish ack"` warn line and the counter.
Either mint a distinct `event_id` per raw row, or do not put a top-level
`event_id` in a raw payload at all.

The id falls back to the **outbox row id** when the stored payload has no
top-level `event_id` **or** carries one that is not a UUID string — the ordinary
shape of a raw `OutboxRecord::stage(id, coords, value)`. The row id is unique
and stable per row, so the crash-replay case the relay actually needs stays
covered; only dedup **across** producers of the same fact is unavailable for such
a payload.

**Delivery stays at-least-once.** The dedup id is a window, not a guarantee: a
replay after the stream's duplicate window has elapsed is delivered again, by
design. Consumers must stay idempotent.

#### Observing a pass — `RelayPass`

```rust,ignore
pub async fn run_once(&self) -> Result<RelayReport, OutboxStoreError>;
pub async fn run_once_detailed(&self) -> Result<RelayPass, OutboxStoreError>;

#[non_exhaustive]
pub struct RelayPass {
    pub picked: usize,
    pub published: usize,
    pub duplicates: usize,
    pub row_id_fallbacks: usize,
    pub failed: usize,
    pub retried: usize,
    pub structural: usize,
    pub min_retry_attempts: Option<u32>,
}
```

`run_once` and `RelayReport` are unchanged; `run_once` now projects a
`RelayPass`, dropping the two counters `RelayReport` has no field for.
`duplicates` is a **strict subset of `published`** — the broker accepted the row,
so the relay marks it published — and counts the frames answered with a duplicate
ack. `row_id_fallbacks` counts the rows whose payload carried no usable envelope
`event_id`; that is a routine, expected outcome for raw-staged payloads, so it is
a counter and not a log line. Both counters are **per pick, not per row**: a row
retried across N passes is counted N times, once per pass that picked it. The
supervised `run()` loop uses `run_once_detailed` internally, so the signals below
fire on the managed loop too.

Each duplicate ack emits one `tracing::warn!` — `"duplicate publish ack"` with
the outbox id, the message id, its source, the stream sequence and the subject —
and increments the counter `outbox_relay_duplicates_total` (`OUTBOX_RELAY_DUPLICATES_TOTAL`)
on the `metrics` facade. The counter is unlabeled: an outbox id would be
unbounded cardinality. It is described **and initialised to zero when an
`OutboxRelay` is constructed**, so the series exists from boot and an alert on a
relay that has never deduplicated reads `0` rather than no-data. The facade is a
no-op until the process installs a recorder, and the `metrics` dependency is
scoped to the `outbox` feature, so a consumer that does not use the outbox gains
nothing.

#### Deployment notes

- **The duplicate window is a stream setting the operator declares**, never the
  lib: this crate binds streams, it does not create or tune them. The NATS
  default is **2 minutes**. If the dedup guarantee is relied on — for instance to
  cover a relay outage longer than that — the window must be **explicit** in the
  stream declaration.
- **Re-staging a row by hand inside the window returns a duplicate ack, not a
  delivery.** That used to be invisible; it is now `RelayPass.duplicates` plus the
  warn line and the counter.
- **A rolling deploy that briefly runs two relays** no longer double-delivers
  inside the window.
- **The window is stream-wide.** Because dedup is per stream and the event stream
  binds every producer's coordinates, a producer that reuses one `event_id` for
  several coordinates loses every frame after the first, inside the window. Mint
  one `event_id` per fact.

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
input (`*`, `>`). On the **single-key read, enumeration and publisher** paths
encode/decode is **fail-closed**: a decode failure is an explicit
`FabricError::Decode` naming the key, never a silent skip. The one documented
exception is `EphemeralAuthWatcher::watch`, which skips an entry it cannot read
rather than failing the loop — see *Surface 3 — Ephemeral Auth over KV*.

### Publisher mechanics

`put` / `update` are a **semantic upsert** (never delete-then-create for an
object that still belongs); `retract` deletes only for a real disappearance.
`reconcile(prefix, desired)` reads the observed entries under a prefix and
applies the minimal op set (put changed/new, delete orphans); `repair_drift` is
the periodic re-run of the same reconcile.

### Compare-and-swap on Published Language

`put` / `update` / `retract` / `reconcile` / `repair_drift` stay **last-writer-wins**:
they ignore the revision chain, and a concurrent writer silently clobbers. That
is the right default for a single-owner mirror, where the publisher is the sole
authority for its prefix. When two writers can race for the same key, the
publisher and the reader also expose the revision-checked path — the same
contract as `EphemeralAuthStore` (see *Surface 3*), plus the new revision
returned by `update_if`, on the `PUBLISHED_LANGUAGE` bucket:

- `PublishedLanguageReader::get_with_revision(&KvKey) -> Result<Option<(V, Revision)>, FabricError>`
  and `PublishedLanguagePublisher::get_with_revision(…)` (same signature) read
  the current value and its `Revision`. A genuinely absent key, and a deleted or
  purged one, both read `Ok(None)`; an undecodable value is `FabricError::Decode`,
  never a silent `None`.
- `PublishedLanguagePublisher::update_if(&KvKey, &V, Revision) -> Result<Revision, FabricError>`
  writes only if the supplied `Revision` is still the last revision for the key,
  and returns the **new** `Revision` on success — chain further writes off that
  value without a re-read. On a mismatch it returns the first-class, matchable
  `FabricError::RevisionConflict { key, expected }` and **nothing is written**.
  A `RevisionConflict` also covers a key another writer has **retracted** — the
  tombstone moved the revision on, so it is indistinguishable from a clobber.
  Re-read with `get_with_revision` before retrying: `None` means the key is
  gone, and a retry loop that does not re-read spins forever on it.
- `PublishedLanguagePublisher::delete_if(&KvKey, Revision) -> Result<(), FabricError>`
  writes a delete tombstone only if the supplied `Revision` is still the last
  revision; on a mismatch it returns the same `FabricError::RevisionConflict` and
  the key stays live. `retract` remains the unconditional delete.

`Revision` is the same opaque newtype used by *Surface 3*: a caller never mints
one. On **this** surface the two calls that hand one out are `get_with_revision`
and a successful `update_if`; `put`, `update`, `retract`, `delete_if`,
`reconcile` and `repair_drift` return none. *Surface 3*'s `update_if` returns
`()` and its revision-returning sibling is named
`update_if_returning_revision` — the two surfaces differ in that one name only.

The read-compare-CAS loop, keyed on an aggregate version carried in the
published DTO:

```rust,ignore
loop {
    let Some((current, revision)) = publisher.get_with_revision(&key).await? else {
        break;
    };
    if current.version >= desired.version {
        break;
    }
    match publisher.update_if(&key, &desired, revision).await {
        Ok(_) => break,
        Err(FabricError::RevisionConflict { .. }) => continue,
        Err(err) => return Err(err),
    }
}
```

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
filter matches nothing and would silently deliver no live updates. This crate
**does not** ship a transformation DSL — filtering and mapping are the caller's.
The raw `watch()` primitive **does not manage the `WatchHealth` channel** — the
supervised loop below is its sole writer (see the health-ownership note there); a
standalone `watch()` caller that wants a health signal drives it through `run()`.

#### Supervised operation — `run()` (the resilient entrypoint)

`bootstrap()` and `watch()` are the raw one-shot primitives: each returns on its
first error, and `watch()` also returns `Ok(())` on a clean stream end. Called
directly and once, a transient broker fault (a missed heartbeat, a node freeze, a
connection drop) therefore kills the mirror **permanently and silently** — the
sink keeps serving its last-good snapshot while the source of truth drifts away,
with no error after the initial return. This is exactly the KV symmetric of the
managed pull-consumer incident that `run_commands`/`run_events` recovery fixed.

`run()` is the supervised entrypoint services should use. It loops forever:
`bootstrap()` → publish `Healthy` → follow the live stream; on **any** watch error
**or** a clean stream end it publishes `WatchHealth::Degraded`, sleeps a bounded
exponential backoff (200 ms doubling to a 30 s cap, the same policy as the pull
path) and then **re-runs `bootstrap()` in full before re-watching**. The wholesale
re-reconciliation is mandatory and deliberate: re-subscribing alone would
silently lose every `Put`/`Delete` that landed during the outage window, so the
recovery path replays the complete scan-project-and-retract-orphans pass — the
mirror reconverges to the bucket's current state on every recovery, missed
`Put`s included, orphans created during the gap retracted. A bootstrap that fails
during recovery (a transient scan error, or a sink/projection failure such as the
DB timeout the incident produced) is retried on the same backoff. **Honest limit:**
this reconvergence guarantee covers the outage gap, **not** the intra-cycle window
between the bootstrap scan and the subsequent `watch_all()` subscription — a `Put`
landing in that (millisecond) handoff window is missed until the next fault
triggers a re-bootstrap. This is a pre-existing bootstrap→watch race, known and
not yet closed.

**Health ownership and semantics.** The supervised loop is the **sole writer** of
the `WatchHealth` channel (the raw `watch()` primitive writes it not at all), so
the signal has one authoritative source. The channel is born `Degraded` — "not
yet converged" is the honest state of a freshly-opened mirror — and is published
`Healthy` **only once a re-bootstrap has completed in full**. It is `Degraded` for
the entire outage and for the whole initial-bootstrap window, so a readiness gate
wired to `health()` never goes ready over an empty or stale mirror.

**Backoff resets on real progress, never on a bare re-bind — mirroring the pull
path.** A recovery counts as progress only when the re-established watch actually
**delivers at least one entry**; a bootstrap that succeeds but is immediately
followed by a 0-entry watch fault (the node-freeze flap) does **not** reset the
backoff, so a persistent instant-fault escalates to the 30 s cap instead of
hammering a full bucket re-scan every 200 ms. A watch that ran and delivered
before faulting resets the floor, so a genuine blip after a long healthy period
recovers fast.

**Failure taxonomy — every error class is retried on the backoff, forever, and
never silently dropped:** a transport/watch error, a clean stream end, a transient
scan error, a sink/projection error, and an undecodable value in the bucket all
keep the loop `Degraded` and retrying until they clear. A decode failure on the
frozen Published-Language wire is a genuine contract breach (a producer/consumer
version skew) that a human must fix, so it wedges reconciliation **loudly**
(`Degraded` health + a `warn` per attempt) rather than being skip-and-forgotten —
the trade-off is that a single poison value blocks convergence for the whole
watched prefix set until it is fixed or purged, which is the correct fail-loud
response for a conformance-frozen wire.

`run()` **never provisions**: the bucket is bound (fail-loud) at `open()`, and the
supervised loop only ever binds and reads it — an absent bucket stays a hard
failure at `open()`, never papered over by the retry loop. `run()` **never
returns** under normal operation — its return type is `std::convert::Infallible`,
so "never returns" is visible in the signature; a caller stops it by **dropping or
aborting the task** (spawn it and `abort()` the `JoinHandle`, or race it in a
`tokio::select!` against a shutdown signal). It is cancel-safe by re-reconciliation
rather than by per-write atomicity: `bootstrap()` and `watch()` are both
restart-from-scratch, so even if a drop interrupts an in-flight sink apply, the
next spawn's `bootstrap()` replays the full reconcile and converges the mirror — a
cancelled cycle loses nothing durably. The raw `bootstrap()` and `watch()`
primitives remain public for tests and advanced callers, but a production mirror
should drive them through `run()`.

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
path on the `EPHEMERAL_AUTH` bucket. The `PUBLISHED_LANGUAGE` bucket has its own
revision-aware path — see *Compare-and-swap on Published Language* under
*Surface 2*.

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
  (`Kv`) and `Decode`, so the caller can drive reuse-detection on it. It returns
  `()`: the new revision is **not** handed back, so a caller chaining a second
  revision-checked operation off this write must re-read.
- `update_if_returning_revision(&KvKey, &V, Revision) -> Result<Revision, FabricError>`
  is the same write with the same conflict semantics, returning the **new**
  `Revision` — the shape `PublishedLanguagePublisher::update_if` has on
  *Surface 2*. Use it to chain (rotate then `delete_if`, or rotate twice) without
  a re-read: a re-read between the two writes is a window in which another writer
  can move the revision on, and the chain then acts on a revision the caller never
  produced. `update_if` is kept unchanged for the callers that do not chain.
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
- `create_with_ttl(&KvKey, &V, Duration)` is `create` with a **per-message TTL**:
  the freshly-created value carries a TTL header so the key expires after `ttl`
  instead of riding the bucket-wide `max_age`. The TTL is set **per message**, so
  it only **shortens** expiry **below** the bucket's `max_age` — it cannot extend a
  key past it. Per-message-TTL support is enabled at provisioning through the
  delete-marker TTL (`subject_delete_marker_ttl` / `limit_markers`), and that marker
  TTL is a **floor**: a per-key TTL shorter than it is silently clamped **up** to it.
  The lib **binds and sets** the per-message TTL; it never creates or configures the
  bucket — gitops declares it with `max_age ≥ longest TTL` **and**
  `subject_delete_marker_ttl ≤ shortest per-key TTL` (otherwise a short
  `create_with_ttl` rides the longer marker TTL). Same matchable
  `FabricError::KeyAlreadyExists { key }` on a live key as `create`.
  **The per-message TTL does not survive a CAS rotation.** The only public rotation
  path, `update_if` → `async-nats` `Store::update`, rewrites the key **without** a
  TTL header, so a key created with `create_with_ttl(family, v, 30d)` and later
  rotated via `update_if` **loses** its per-key TTL and reverts to the bucket
  `max_age` — there is no public CAS-update-with-TTL path in `async-nats` (0.48 and
  0.49.1; `update_maybe_ttl` is private). Position `create_with_ttl` on its
  truly-covered case: a **one-time code** (created once, never rewritten, expires at
  `ttl`). For a **refresh family** whose TTL must be stable across rotations, the
  effective TTL is the bucket `max_age`, not the initial `create_with_ttl` value —
  size the bucket `max_age` to the family lifetime and let rotations ride it. There
  is no `put_with_ttl`: an unconditional last-writer-wins write carrying a per-key
  TTL is not part of the public `async-nats` KV surface at any current version (only
  the CAS-flavoured `create_with_ttl` is), so use `create_with_ttl` for the one-time
  code lifecycle (see the CHANGELOG note).
- `keys(&KvPrefix) -> Result<Vec<KvKey>, FabricError>` enumerates the live keys
  under the prefix **without decoding** values — tombstoned (deleted / purged /
  TTL-expired) keys are excluded, consistent with `get_with_revision`. Identical
  prefix-scoped shape to `PublishedLanguageReader::keys`.
- `entries(&KvPrefix) -> Result<BTreeMap<KvKey, V>, FabricError>` enumerates the
  live keys under the prefix **and decodes** each value, **fail-closed** — an
  undecodable value surfaces as a `FabricError::Decode` naming the key, never a
  silent skip — also excluding tombstones. Identical shape to
  `PublishedLanguageReader::entries`.
- `watcher() -> EphemeralAuthWatcher<V>` opens a change watch over the bound
  bucket so a service reacts to "this family was just revoked / rotated elsewhere"
  **without polling**. Each call mints a **fresh watcher with its own
  health channel**, so bind one watcher and reuse it: a `health()` taken from one
  `store.watcher()` while another `store.watcher()` runs the watch is a receiver
  nothing ever publishes to, frozen on its initial `Degraded`.
  A watcher also exposes `progress() -> WatchProgressReceiver`, a
  `tokio::sync::watch` of the `#[non_exhaustive] WatchProgress { changes,
  skipped }` counters — entries handed to the handler, and entries skipped as
  unreadable — with `observed()` for their sum. Like `health()`, it is per
  watcher instance.
  `EphemeralAuthWatcher::watch(on_change)` runs the watch loop, invoking the
  caller's `FnMut(EphemeralAuthChange<V>)` per change —
  `EphemeralAuthChange::Set { key, value }` for a put and
  `EphemeralAuthChange::Removed { key }` for a delete / purge. The raw
  `async_nats` `Entry` is never handed to the caller — only the typed change.
  **An entry this consumer cannot read is skipped, not fatal.** A value that
  fails to decode into `V`, or a key `KvKey` rejects, is dropped with one
  `tracing::warn!` carrying exactly four fields — `surface = "ephemeral-auth"`,
  `key` (the raw KV key), `reason` (the static discriminant `"undecodable
  value"` or `"invalid key"`) and `value_len` (the value's byte length) — then
  counted on `progress()`, and the watch **keeps running**; health stays
  `Healthy`, because the stream is alive. **The underlying `FabricError` is
  deliberately never logged:** a `serde_json` message embeds the offending value
  fragment on an `invalid type` / `invalid value` / `unknown variant` failure,
  and on this bucket that fragment is credential state. This is the
  deliberate opposite of `PublishedLanguageConsumer`, which wedges loudly on an
  undecodable value: that loop owns a local mirror whose convergence a poison
  entry would silently falsify, while this one only forwards notifications and a
  single skew-written entry must not cost the consumer every other change. A
  poison entry here is therefore **not** surfaced as an error to the caller — it
  is visible on `progress()` and in the warn line only, and a consumer that must
  react to it watches `WatchProgress::skipped`.
  `EphemeralAuthStore::entries` and `get_with_revision` are unchanged and stay
  fail-closed: reading a poison key on purpose is still an explicit
  `FabricError::Decode`.
  `EphemeralAuthChange::Removed` covers **TTL-expiry only when the bucket is
  declared with delete-marker TTL** (`limit_markers` / `allow_msg_ttl`): a
  TTL-expired key surfaces as a delete/purge marker on the watch only if the bucket
  emits one, so a consumer reacting to a revoke-by-expiry is coupled to that gitops
  provisioning constraint — the lib never provisions the bucket and cannot
  guarantee the marker. `watch()` is the **raw one-shot primitive**: it loops
  until error or stream-end and has **no cancellation token**, so a caller stops
  it by **dropping the `watch` future** (it is cancel-safe — a read-only stream
  plus a health channel, no partial write) under a `tokio::select!` /
  `CancellationToken`. A production consumer drives it through `run()` instead —
  see *Supervised operation* below.
- `status()` exposes the **bound bucket's cached KV state** in the bind-existing
  posture — it reads `async_nats`'s locally-cached stream info and does **not**
  round-trip the broker, so it is **not** a live reachability probe and must not
  back a liveness gate. The fail-loud liveness check is `open()` (the real bind
  round-trip): if the bucket is unreachable at startup, `open()` fails and
  readiness stays DOWN.

`Revision` is an opaque newtype over the NATS KV sequence — the caller reads it
from `get_with_revision` and passes it back to `update_if`,
`update_if_returning_revision` or `delete_if`. A caller never mints a `Revision`
by hand. On **this** surface exactly two calls hand one out:
`get_with_revision` and a successful `update_if_returning_revision`; `update_if`
returns `()`, and `create`, `create_with_ttl`, `put`, `delete` and `delete_if`
return no revision either — so a chain that starts at a `create` needs one
`get_with_revision` before its first revision-checked write.

### Supervised operation — `run()` (resubscribe-only)

`watch(on_change)` returns on its **first** stream error, and returns `Ok(())` on
a clean stream end. Called directly and once, a transient broker fault (a missed
heartbeat, a node freeze, a connection drop) therefore kills the watch
**permanently and silently**: a service watching this bucket for cross-instance
revocation loses the watch on the first fault and nothing restarts it — it stops
seeing "this family was revoked / rotated elsewhere", with no error after the
initial return, until the process restarts. `run(on_change)` is the supervised
entrypoint such a service should use.

`run()` is the same loop as `PublishedLanguageConsumer::run()` — reconcile →
publish `Healthy` → follow the live stream; on **any** watch error **or** a clean
stream end publish `WatchHealth::Degraded`, sleep the same bounded exponential
backoff (200 ms doubling to a 30 s cap) and reconcile again before re-watching —
with the same `std::convert::Infallible` return (a caller stops it by **dropping
or aborting the task**), the same never-provisions posture, and the same
progress rule: a recovery resets the backoff floor only when the re-established
watch **delivered at least one change**, so an instant-fault flap escalates to
the cap instead of hammering the broker.

**One deliberate difference: the reconcile step is a bucket-presence check, not a
wholesale re-reconciliation — the recovery is *resubscribe-only*.** The
Published-Language loop re-runs a full `bootstrap()` because it owns a **local
mirror** that would silently drift over the outage gap. `EPHEMERAL_AUTH` has no
such mirror: it is compare-and-swap-written and TTL-bounded, the bucket **is**
the truth, and the watcher's consumer reacts to changes rather than holding
derived state — there is nothing to rebuild. So recovery only re-arms the
subscription, gated on the bucket still being there. That gate is a **live
`STREAM.INFO` round-trip**, deliberately **not** `status()` (which reads
`async_nats`'s cached stream info and could never observe a bucket that went
away): a deleted bucket keeps the loop `Degraded` with a `warn` per attempt —
fail-loud, never papered over — and the loop re-arms by itself once the bucket is
back.

**An unreadable entry does not restart the loop.** Because `watch()` skips it
rather than returning, a rolling deploy in which a schema-skewed writer keeps
publishing values this consumer cannot decode does **not** flap the supervisor:
the watch stays up, every readable change behind the poison entry is still
delivered, and `WatchProgress::skipped` climbs. Skipped entries also count as
**progress**, so an attempt that only skipped still resets the backoff floor — a
stream that is demonstrably alive never escalates to the 30 s cap.

**Honest limit — the outage gap is lost, not replayed.** Resubscribe-only means a
`Set`/`Removed` that lands while the watch is down is **never** delivered: there
is no catch-up scan and no replay. That is sufficient for the canonical consumer
because a rotation/reuse decision re-reads the key under CAS at decision time
(`get_with_revision` → `update_if`), so a missed notification delays a reaction
without corrupting a decision. A consumer that keeps **derived** state off this
watch must re-read the bucket after a `Degraded` → `Healthy` transition **and on
any increase of `WatchProgress::skipped`**; it must not treat the change stream
as gap-free. The two triggers cover two different loss classes: the outage gap
shows on `health()`, while a skipped `Set` — a revocation or a rotation this
consumer could not read — never moves health, so `progress()` is its only
signal.

**Health.** Under `run()` the channel is born `Degraded`, is published `Healthy`
only once the presence check has passed, and returns to `Degraded` for the whole
fault + backoff window. The raw `watch()` primitive keeps driving the same
channel for the standalone caller, and its transitions coincide with the loop's,
so the two never contradict each other. A key/decode error is **not** one of
those transitions: it is skipped, not returned, so it never moves health — an
alive watch reads `Healthy` however many entries it had to skip, which is
precisely why `WatchProgress::skipped`, not `health()`, is the recovery trigger
for a dropped entry. **`health()`
reflects the loop's
state, not the task's liveness** — the two documented stop modes leave the
channel frozen on its last published value: an aborted or dropped task keeps
whatever it last set (`Healthy`, if it was following), and a **panicking handler**
propagates out of `run()` and kills the task the same way (the `tokio::sync`
mutex holding the handler does not poison, so nothing marks the loop down). A
readiness gate must therefore treat a sender-dropped receiver — `rx.changed()`
returning `Err` — as DOWN, and the caller must keep its handler panic-free. That
signal only exists if the supervising task **owns** the watcher: move the watcher
into the task, because a watcher held elsewhere never drops the sender and the
gate would read a frozen `Healthy` forever.

**The handler is reused, never cloned.** The caller's `FnMut` is held by the loop
and handed to each attempt behind a mutex, so a handler carrying state (a counter,
a dedup set, a channel sender) keeps it across every re-arm instead of restarting
from a fresh copy on recovery.

## Generic mechanics vs caller seams (summary)

| Generic (this crate owns)                              | Caller seam                                  |
| ------------------------------------------------------ | -------------------------------------------- |
| the v1 grammar, fixed streams, fixed bucket            | the business coordinates + payload type      |
| subject rendering, durable create-or-bind + its config | the durable name                             |
| readiness subject-coverage matching against the stream binding | the coordinate to probe                 |
| reconcile op computation, orphan detection             | the desired set                              |
| bootstrap scan + watch loop, fail-closed codec         | the prefix selection                         |
| exact-key single-key read (`PublishedLanguageReader`)  | the `KvKey` to read                          |
| prefix enumeration (`PublishedLanguageReader::keys`/`entries`) | the `KvPrefix` to scan                 |
| compare-and-swap KV (`EphemeralAuthStore`, `PublishedLanguagePublisher`, `Revision`) | the `KvKey`, the value, the observed revision |
| per-key TTL on create, enumeration, change-watch and its supervised re-arm, and the watch's health/progress signals (`EphemeralAuthStore`, `EphemeralAuthWatcher::run`, `health`, `progress`) | the `Duration`, the `KvPrefix`, the `on_change` handler, the reaction to a rising `skipped` |
| the copy-filter *mechanism*                            | the `Fn(&V) -> bool` predicate               |
| the projection *mechanism* (full `V` to the sink)      | the `ProjectionSink<V>` (what to persist)    |
| the outbox dedup id (envelope `event_id`, row id fallback) and the duplicate-ack signals | the stream's `duplicate_window`, declared out of band |

## Why

| Thing | Why it is the way it is |
| ----- | ----------------------- |
| `IntegrationConsumer::drain()` is `async` though it currently only drops the pull stream | The signature reserves a future awaiting drain (in-flight-ack / unsubscribe flush) and avoids a later breaking sync→async change. |
| `verify_*_durable` keeps a `durable` parameter it no longer uses | It used to create that durable (five phantom consumers accumulated in prod); the probe was fixed in place so every existing readiness call site keeps compiling and simply stops leaving a consumer behind. |
| `EphemeralAuthWatcher::run()` reconciles by probing the bucket, while `PublishedLanguageConsumer::run()` re-bootstraps its whole mirror | `EPHEMERAL_AUTH` is CAS-written and TTL-bounded and its consumers hold no local replica — the bucket is the truth, re-read under CAS at decision time — so there is nothing to rebuild and recovery only has to re-arm the subscription; the probe exists solely to fail loud when the bucket itself is gone. |
| `RelayPass` exists beside `RelayReport` instead of `RelayReport` gaining two fields | `RelayReport` is a plain (non-`#[non_exhaustive]`) struct in the published API, so adding a field to it is a breaking change for every consumer that constructs or exhaustively destructures one. |
| `EphemeralAuthStore` has both `update_if` (returns `()`) and `update_if_returning_revision` | `update_if` shipped in `1.2.0` returning `()`; changing its return type is a breaking change, so the chainable form is an additive sibling rather than a corrected signature. |
| `EphemeralAuthWatcher` skips an entry it cannot read while `PublishedLanguageConsumer` wedges on one | The consumer owns a local mirror a poison entry would silently falsify, so it must stop; the watcher only forwards notifications, and stopping cost it every other change on the bucket plus a backoff walk to the cap on each retry. |
| A skipped entry is signalled on `progress()` and never on `health()` | Health tracks whether the subscription is alive, and it is: degrading on a value one consumer cannot read would make a schema-skewed writer look like a broker outage and would flap every readiness gate on the bucket. The loss is real but it is a per-entry loss, so it gets a per-entry counter, and `WatchProgress::skipped` is what a consumer holding derived state must watch. |
| A skipped entry can never leave a stale key in a consumer's cache | `change_from` rejects an unusable key for both `Set` and `Removed`, and `KvKey::new` is deterministic — so a `Removed` is skipped only for a key whose `Set` was skipped too, and no handler ever saw it. The only asymmetric case is an undecodable value, which fails on `Set` alone: its `Removed` still arrives. Skipping therefore drops notifications, never leaves a consumer holding a key it can no longer be told about. |
| The warn on a skipped entry logs a static `reason` plus `value_len`, never the `FabricError` | `FabricError::Decode` carries the raw `serde_json` message, which quotes the offending value fragment on an `invalid type` / `invalid value` / `unknown variant` failure; on `EPHEMERAL_AUTH` that fragment is credential state, so the error string cannot reach a log sink. |
| The outbox dedup id prefers the envelope `event_id` over the outbox row id | It is the same key hand-rolled relays elsewhere already use, so the platform keeps **one** dedup keyspace; carrying it in a dedicated column would mean a breaking migration on a table whose DDL is consumer-owned. |

## Dependency

```toml
br-util-nats-fabric = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-nats-fabric", tag = "v1.3.0", version = "1.3.0" }
# with the transactional outbox:
# br-util-nats-fabric = { git = "...", package = "br-util-nats-fabric", tag = "v1.3.0", version = "1.3.0", features = ["outbox"] }
```
