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
key rendering. The KV/PG round-trip —
real-NATS + real-PG orphan-delete, extension survival, pass→fail orphan, the
users-only scope, the absent-manifest fail-closed — is the **conformance-directory**
battery in `br-e2e-harness` (a post-tag follow-up there, out of scope for this
crate).

## Why

| Thing | Why it is the way it is |
|---|---|
| No KV engine in this crate | The generic upsert/retract/reconcile/orphan-delete/bootstrap/watch is `br-util-nats-fabric`'s; this crate keeps only the directory *meaning* (keys, DTOs, schema, recompose). |
| `DirectorySource` is the only publisher seam | The project owns its domain→`Published*` mapping; the kit owns the reconcile mechanism. |
| The user sink re-upserts on every projected entry | An idempotent `ON CONFLICT … DO UPDATE` over the KV scan is cheaper than re-reading the local row to diff; only deletes need the observed-vs-desired set. |
| Group upsert replaces its junction rows in one transaction | A membership change is atomic and idempotent under redelivery. |
| Memberships are group-derived, `user_id` has no FK | A membership is recomposed straight from the group's `member_ids`, independent of whether that user has a `known_users` row. The user, group and service-account watches are independent streams with no inter-entity re-trigger, so a group can project before one of its members' user entry (or a member may be filtered out / never published under a scoped roster). A FK + member-existence guard silently dropped such a row and never re-projected the group when the user later arrived (`is_member` stayed wrong). So `known_user_group.user_id` carries no FK; the group reconcile/watch replaces a group's rows from its `member_ids` (delete-then-insert) — order-independent convergence. A member with no `known_users` row is legitimate, not an orphan; `is_member` is correct regardless, while `resolve_user` returns `None` for a filtered/not-yet-projected user (the expected scoped behavior). |
| Manifest absent = fail-closed, not empty roster | Treating an absent manifest as empty orphan-deleted every local row (a PII purge) when a consumer merely booted ahead of identity. Fail-closed leaves the projection intact. |
| Readers resolve over `DirectorySnapshot`, a pure projection | Resolution + auto-degrade stay unit-testable with no I/O; the PG-backed readers mirror the semantics, proven in the e2e conformance battery. |
| `delete_group` relies on `ON DELETE CASCADE` | Purges the junction via the `known_user_group` group FK — the contract relies on it. |
