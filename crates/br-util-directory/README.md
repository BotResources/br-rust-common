# br-util-directory

Publisher + consumer **kit** for the identity **Published Language** (the read
contract frozen in `br-core-directory`). Tier `util`: it carries the
directory-specific *meaning* — the `identity/...` key prefixes, the
`Published{User,Group,ServiceAccount}` DTOs, the `known_*` projection schema,
member recomposition — over the **generic** Published-Language KV mechanics
owned by `br-util-nats-fabric`. It owns no KV engine of its own.

The identity bounded context is the **only writer** of the KV roster; every
other service is a **reader**. PII (email/name) lives in KV, so **deletion must
propagate** — both sides reconcile by **orphan-delete**, never by wipe.

## Built on the NATS Fabric (no KV engine here)

This kit holds **no** `async_nats` KV `Store`, no `put`/`delete`/`keys` loop, no
`reconcile` op computation. All of that is the generic
`br_util_nats_fabric::{PublishedLanguagePublisher, PublishedLanguageConsumer}`
over the fixed `PUBLISHED_LANGUAGE` bucket. `DirectoryPublisher::open(&fabric)`
and `DirectoryProjector::new(fabric, pool)` construct from a `&Fabric`; the
fabric binds the bucket **internally** and **fails loud** if it is absent. The
raw `Store` is never exposed, so every key path goes through a validated
`KvKey` / `KvPrefix`. This crate maps the directory's `Uuid`-keyed entities onto
those validated keys and supplies the typed values; the fabric does the
upsert / retract / reconcile / orphan-delete / bootstrap-scan / watch.

## One crate, feature-gated (the dependency asymmetry is real)

```text
default  = []                                     # neither side; pulls no I/O dep
publisher = …                                     # KV publish only, NO Postgres
consumer  = … + br-util-postgres + sqlx + tokio   # KV -> PG projection
```

A consumer service that only reads the roster does not pull the publisher path;
identity, the only publisher, does not pull `br-util-postgres`. The fabric
dependency is shared by both (the error type and key helpers are common).

## No auto-provisioning — fail loud (hard rule)

The kit **never creates the KV bucket** (the fabric never provisions) and never
creates the `known_*` schema — that is the migration the caller runs at deploy
time. A missing bucket surfaces as a typed `DirectoryError::Fabric`.

## Publisher (feature `publisher`, mounted in identity)

The project supplies the **seam** — its source of truth — by implementing:

```text
#[async_trait]
trait DirectorySource {
    fn manifest(&self) -> DirectoryMeta;
    async fn desired_users(&self) -> Result<BTreeMap<Uuid, PublishedUser>, DirectoryError>;
    async fn desired_groups(&self) -> Result<BTreeMap<Uuid, PublishedGroup>, DirectoryError>;
    async fn desired_service_accounts(&self)                              // default = empty
        -> Result<BTreeMap<Uuid, PublishedServiceAccount>, DirectoryError>;
}
```

`DirectoryPublisher::open(&fabric)` provides the **mechanism**:

- `reconcile(&source)` — boot-time: per entity (users, groups, service
  accounts) it calls the fabric's `reconcile(prefix, desired)` — put new/changed,
  **DELETE orphans** under that prefix — then writes the `identity/_meta`
  manifest. An entity the manifest does not declare reconciles against an empty
  desired set, so any stale key is orphan-deleted (degrading propagates the PII
  deletion).
- `publish_user` / `retract_user` / `publish_group` / `retract_group` /
  `publish_service_account` / `retract_service_account` — incremental
  single-entity touches on a domain event.
- `write_meta` — (re)publish the manifest.

## Consumer (feature `consumer`, mounted in generic services)

- **`connect_pool(database_url)`** — the TLS-validated `PgPool` for the
  `known_*` projection, built through `br_util_postgres::init_pool`.
- **`migrate(pool)`** — creates `known_users` (incl. a `jsonb extensions`
  column), `known_groups`, the junction `known_user_group`, and
  `known_service_accounts` (`migrations/0001_known_directory.sql`).
- **`DirectoryProjector::new(fabric, pool)`** (or `with_config(fabric, pool,
  config)`) — the KV→PG projector over fabric consumers:
  - `reconcile()` — boot-time: read `identity/_meta`; if **absent**, fail closed
    with `DirectoryError::ManifestAbsent` (see below) and project **nothing**.
    Otherwise run, per consumed entity, the fabric consumer's `bootstrap()`
    (scan-and-project + **orphan-delete within that prefix** against the sink's
    own `known_keys`). Returns the `DirectoryMeta` it read.
  - `watch()` — reads `identity/_meta` **once** at start (fail-closed with
    `DirectoryError::ManifestAbsent` if absent), then runs the per-entity fabric
    watches concurrently; each live KV update projects or retracts through the
    entity's sink. The manifest is **not** hot-reloaded: activating a new entity
    (a manifest republish that newly declares groups or service accounts)
    requires a consumer restart — intentionally not done live.
- **Denormalized-KV → normalized-PG.** The group sink recomposes the
  denormalized `PublishedGroup { name, member_ids }` into `known_groups` plus one
  `known_user_group` row per member, in one transaction (delete the group's old
  junction rows, insert one row per `member_id`). Membership rows are recorded
  for **every** `member_id`, independent of whether that user is currently in
  `known_users` — `known_user_group.user_id` carries no FK, so a group projected
  before (or without) one of its members still converges: the membership is
  correct as soon as the group projects, and `resolve_user` returns the user once
  it arrives. A member with no `known_users` row is legitimate under a scoped
  roster, not an orphan (see #69 — group deletion CASCADEs the junction via the
  `group_id` FK).
- **Typed readers carry the id** over `DirectorySnapshot`: `resolve_user`,
  `user_extensions`, `is_member`, `group_name`, `resolve_service_account`.
  `DirectorySnapshot` / `KnownUser` are an **in-memory** projection the
  **consuming service** populates and owns (the kit ships no PG-backed reader over
  the `known_*` tables here — that mirror lives on the consumer side); the
  `extensions` field on `KnownUser` is the consumer-extracted payload selected by
  `extract_user_extensions`. **Auto-degrade**: a snapshot built from a manifest
  that does not declare an entity returns `None` / `false` / empty from that
  entity's readers.

### Change detection, impacts and the stager seam (1.3.0)

Every sink write is now **change-detecting** and **transactional**:

- `known_users` / `known_service_accounts` / `known_groups` upsert with
  `ON CONFLICT (…) DO UPDATE SET … WHERE (t.cols…) IS DISTINCT FROM (EXCLUDED.cols…)`;
  `rows_affected() == 0` means the row was already identical, so **no row
  version is written** (no dead tuple, no bloat) and nothing downstream is
  notified.
- A **group** is changed when its name changed **or** its recomposed member set
  differs from the set read under `FOR UPDATE` **after the group-row upsert**
  (`SELECT user_id FROM known_user_group WHERE group_id = $1 … FOR UPDATE`,
  compared as a set). Memberships are rewritten only when the sets differ.
- A `retract` counts as a change only when a row was actually deleted.

An adopter that has to react to a roster change registers a stager:

```text
pub const USER_NAMESPACE: &str            = "identity.user";
pub const GROUP_NAMESPACE: &str           = "identity.group";
pub const SERVICE_ACCOUNT_NAMESPACE: &str = "identity.service_account";

pub struct ForeignRef;                       // (namespace, key), validated at construction
impl ForeignRef {
    pub fn new(namespace: &str, key: &str) -> Result<Self, DirectoryError>;
    pub fn namespace(&self) -> &str;
    pub fn key(&self) -> &str;
}

#[non_exhaustive]
pub enum Impact { ForeignChanged { foreign: ForeignRef } }

#[async_trait]
pub trait ImpactStager: Send + Sync {
    async fn stage_in(&self, conn: &mut sqlx::PgConnection, impacts: &[Impact])
        -> Result<(), DirectoryError>;
}

impl DirectoryProjector {
    pub fn with_impact_stager(self, stager: Arc<dyn ImpactStager>) -> Self;   // beside new / with_config
}
```

`DirectoryProjector::new(fabric, pool).with_impact_stager(stager)` and
`with_config(fabric, pool, config).with_impact_stager(stager)` are both valid —
the builder composes with either constructor. When a stager is registered and
**only when the row actually changed**, the sink calls `stage_in` **inside the
same transaction** as the roster write, with one
`Impact::ForeignChanged { foreign }` carrying the entity's namespace and its
`Uuid` rendered as the key. The mirror produces no other variant: it can only
say *this foreign fact changed*; noun/resource addressing belongs to whatever
consumes the impacts. `Impact` is `#[non_exhaustive]`.

The service-engine brief names the same value `ForeignKey`; this crate ships it
as `ForeignRef` with the identical `(namespace, key)` shape, so the mapping
downstream is a rename and nothing else — no field is added, dropped or
reinterpreted. Accepted charset, so an engine-side validator can be made to
match: **namespace** = 1–64 bytes of `[a-z0-9._]`, no leading or trailing `.`
and no `..`; **key** = 1–256 bytes with no control character and no whitespace.
The three sink namespaces above satisfy it, and the key the sinks emit is always
a hyphenated lowercase `Uuid`.

`stage_in` returns `DirectoryError::Stager(source)` — the adopter boxes its own
error as the source. The sink does not interpret it: it rolls the transaction
back and surfaces the error unchanged to `reconcile()` / `watch()`.

**Adopter note — grants.** A stager writes to the **adopter's own** tables in
the sink's transaction, so those tables need a grant on the runtime app role
(the least-privilege grant migration each service owns). Without it the
stager fails and, because it runs inside the transaction, **the roster upsert
rolls back with it** — the projection stops converging. That coupling is the
point (an impact is never staged for a write that did not commit), but it is a
new way for a missing grant to break the mirror.

Without a stager the behaviour is identical to `1.2.0` apart from the
transaction wrapper and the suppressed no-op writes.

### Two supervision signals (1.3.0)

```text
impl DirectoryProjector {
    pub fn progress(&self) -> ProjectorProgressReceiver;   // watch::Receiver<ProjectorProgress>
    pub fn health(&self)   -> WatchHealthReceiver;         // br_util_nats_fabric::WatchHealth
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProjectorProgress { pub changes: u64 }
```

- `progress()` — a monotonic counter bumped **once per committed change**, on
  exactly the same predicate that decides whether to stage an impact. An
  unchanged upsert never bumps it; a rolled-back transaction never bumps it.
  Both `reconcile()` and `watch()` feed it.
- `health()` — the **worst-of** composition over the streams that are active for
  this projector (users always; groups when the consumption scope includes them;
  service accounts when the producer manifest declares them). A stream is
  `Healthy` from the moment its consumer is **bound to the bucket** (the fabric
  fails loud if it is absent) until its `watch()` call **returns**, i.e. while
  the call is in flight; it is `Degraded` before `watch()` starts, after it
  returns, and for the whole life of a projector that never watches. That is
  exactly what the crate can observe: `PublishedLanguageConsumer::watch()`
  reports no "watcher established" edge, so `Healthy` means *bound and not yet
  failed*, never *has received an event*.

**Supervision is the caller's.** This crate has no loop, no backoff and no
re-reconcile: `watch()` still returns on the first stream error. The two signals
exist so a supervisor above the crate can decide when to restart it and when to
report readiness.

### Missing manifest is DEGRADED, never a purge (#69)

A missing `identity/_meta` no longer means "empty roster → delete every local
row". `reconcile()` / `watch()` treat an absent manifest as **fail-closed**
(`DirectoryError::ManifestAbsent`): the projection is left untouched and the
caller surfaces a degraded/unready signal. A consumer that boots ahead of
identity's first reconcile therefore **does not** flush its projection.

### Consumer-owned roster control (#59)

Two **seams** on `DirectoryConsumerConfig` (defaults preserve the prior
name-only / keep-all behavior), wired into the fabric consumer's projection sink
and copy-filter:

- `extract_user_extensions(impl Fn(&PublishedUser) -> PersistedExtensions)` —
  selects which extension payload to persist into the `jsonb extensions` column;
  **default keeps nothing**. A consumer scopes its roster discriminator (e.g.
  `is_platform_member`) from fields identity **already** publishes in
  `extensions` — no publisher change.
- `filter_users(impl Fn(&PublishedUser) -> bool)` — which users are copied at
  all; **default keep-all**. A user that flips pass→fail is **orphan-deleted**
  on the next reconcile and on the watch update carrying the failing value (the
  fabric's copy-filter mechanism).

### Consumer-declared consumption scope (#63)

`DirectoryConsumerConfig::scope(ConsumptionScope)` — **independent of the
producer manifest**:

- `UsersOnly` — only `known_users` is projected and watched; no group prefix is
  scanned, no group tables are touched.
- `UsersAndGroups` (**default**) — users + groups.

`UsersOnly` bounds the dependence on the **group** tables: with no group-key
handling there is no scan of, and no crash on the absence of, `known_groups` /
`known_user_group`. Service accounts are governed by the producer manifest
(projected when it declares them), orthogonal to the users/groups scope.

## Tenancy-agnostic (hard rule)

Like `br-core-directory`, the kit names **no** orgs / tenancy concept. It
reads/writes the core contract and the opaque `extensions` bag generically;
`organization_id` is a project extension a consumer reads on its own side via
`PublishedUser::extension("…")` and persists through `extract_user_extensions`.

## Tested here vs deferred to e2e

Unit tests cover the **pure logic**, no I/O: `member_rows` (recompose),
`DirectoryConsumerConfig` (default keep-nothing / keep-all, custom
extract / filter), `DirectorySnapshot` (resolve / extensions / membership /
service accounts, **auto-degrade**, and **order-independent convergence**: a
group's membership is correct even when set before the member user is projected),
key rendering, `ForeignRef` validation, the progress channel and the
worst-of health composition. `tests/projector_e2e.rs` covers the projector
against **real NATS + real Postgres** (`#[ignore]`, gated on `NATS_URL` and
`TEST_DATABASE_URL`, each test in its own Postgres schema): the no-op upsert
(proven by an unchanged `xmin`), the single staged impact on a real change, the
stager-failure rollback, the group name/member-set predicate, and the
stager-less path. The rest of the KV/PG round-trip — orphan-delete, extension
survival, pass→fail orphan, the users-only scope, the absent-manifest
fail-closed — is the **conformance-directory** battery in `br-e2e-harness`.

## Install

```toml
[dependencies]
br-util-directory = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-directory", tag = "v1.3.0", version = "1.3.0", features = ["consumer"] }
# the identity side, which writes the roster:
# br-util-directory = { git = "...", package = "br-util-directory", tag = "v1.3.0", version = "1.3.0", features = ["publisher"] }
```

`default = []`: pick `consumer` (the `known_*` projection + readers),
`publisher` (the roster writer), or both.

## Why

| Thing | Why it is the way it is |
|---|---|
| No KV engine in this crate | The generic upsert/retract/reconcile/orphan-delete/bootstrap/watch is `br-util-nats-fabric`'s; this crate keeps only the directory *meaning* (keys, DTOs, schema, recompose). |
| `DirectorySource` is the only publisher seam | The project owns its domain→`Published*` mapping; the kit owns the reconcile mechanism. |
| The sinks upsert with an `IS DISTINCT FROM` guard instead of blind `DO UPDATE` | The projector re-projects the whole prefix on every reconcile, so a blind upsert rewrote every row on every boot — dead tuples, and no way to tell a real roster change from a re-scan. The guard makes `rows_affected()` the single, NULL-correct definition of "changed", which is what both the impact and the progress signal key off. |
| Group upsert replaces its junction rows in one transaction | A membership change is atomic and idempotent under redelivery. |
| The group row lock, not the membership `FOR UPDATE`, is what serialises two replicas | The `SELECT … FROM known_user_group … FOR UPDATE` locks nothing when the group has no membership rows yet, so it cannot be the serialisation point. The `known_groups` upsert is: `ON CONFLICT … DO UPDATE` takes the row lock on the conflicting row before evaluating its `WHERE`, so even a no-op (`WHERE` false) upsert holds it, and it is taken first in the transaction. The membership `FOR UPDATE` then only pins the rows the sink is about to rewrite. |
| The group sink reads its member set back before rewriting it | Delete-then-insert makes `rows_affected()` meaningless for memberships (it reports rows touched, not a difference). Reading the current set under `FOR UPDATE` and comparing it with the recomposed one is the only honest "changed" for a group, and it also skips the rewrite when nothing moved. |
| A stager failure rolls the roster write back | An impact that outlives its write is a lie to whoever reacts to it, and a write with no impact is a silently-missed reaction. Sharing one transaction is what makes the pair atomic; the cost is the grant note above. |
| Memberships are group-derived, `user_id` has no FK | A membership is recomposed straight from the group's `member_ids`, independent of whether that user has a `known_users` row. The user, group and service-account watches are independent streams with no inter-entity re-trigger, so a group can project before one of its members' user entry (or a member may be filtered out / never published under a scoped roster). A FK + member-existence guard silently dropped such a row and never re-projected the group when the user later arrived (`is_member` stayed wrong). So `known_user_group.user_id` carries no FK; the group reconcile/watch replaces a group's rows from its `member_ids` (delete-then-insert) — order-independent convergence. A member with no `known_users` row is legitimate, not an orphan; `is_member` is correct regardless, while `resolve_user` returns `None` for a filtered/not-yet-projected user (the expected scoped behavior). |
| Manifest absent = fail-closed, not empty roster | Treating an absent manifest as empty orphan-deleted every local row (a PII purge) when a consumer merely booted ahead of identity. Fail-closed leaves the projection intact. |
| Readers resolve over `DirectorySnapshot`, a pure projection | Resolution + auto-degrade stay unit-testable with no I/O; the PG-backed readers mirror the semantics, proven in the e2e conformance battery. |
| `delete_group` relies on `ON DELETE CASCADE` | Purges the junction via the `known_user_group` group FK — the contract relies on it. |
