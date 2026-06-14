# br-util-directory

Publisher + consumer **kit** for the identity **Published Language** (the read
contract frozen in `br-core-directory`). Tier `util`: it carries I/O — NATS KV
on the publisher side, NATS KV + Postgres on the consumer side — and builds on
`br-core-directory` for the wire types and KV-key conventions.

The identity bounded context is the **only writer** of the KV roster; every
other service is a **reader**. PII (email/name) lives in KV, so **deletion must
propagate** — both sides reconcile by **orphan-delete**, never by wipe.

## One crate, feature-gated (the dependency asymmetry is real)

```text
default  = []                                     # neither side; pulls no I/O dep
publisher = …                                     # KV only, NO Postgres
consumer  = … + br-util-postgres + sqlx           # KV -> PG projection
```

A consumer service that only reads the roster does not pull the publisher path;
identity, the only publisher, does not pull `br-util-postgres`. The split is
enforced in `Cargo.toml`: `--features publisher` does **not** compile
`br-util-postgres`/`sqlx`; `--features consumer` does.

## No auto-provisioning — fail loud (hard rule)

The kit **never creates the KV bucket**. `DirectoryPublisher::new` /
`DirectoryProjector::new` take an already-bound `async_nats::jetstream::kv::Store`
— resolved (and its absence turned into a readiness-DOWN) by the caller's
declared-infra boot. The kit assumes the bucket exists; if it does not, the KV
call fails and surfaces as a typed `DirectoryError::Kv`. Same stance for the PG
side: the `known_*` schema is created by the migration **the caller runs at
deploy time**, never on demand.

## Publisher (feature `publisher`, mounted in identity)

The project supplies the **seam** — its source of truth — by implementing:

```text
#[async_trait]
trait DirectorySource {
    fn manifest(&self) -> DirectoryMeta;
    async fn desired_users(&self) -> Result<BTreeMap<Uuid, PublishedUser>, DirectoryError>;
    async fn desired_groups(&self) -> Result<BTreeMap<Uuid, PublishedGroup>, DirectoryError>;
}
```

`DirectoryPublisher` provides the **mechanism**:

- `reconcile(&source)` — boot-time: read the whole KV bucket, diff against the
  source's desired state, apply **minimal touches** (put new/changed, **DELETE
  orphans**), then write the `identity/_meta` manifest. When the manifest does
  not declare `groups`, desired-groups is treated as empty, so any stale group
  key in KV is orphan-deleted (degrading propagates the PII deletion).
- `publish_user` / `retract_user` / `publish_group` / `retract_group` —
  incremental single-entity touches on a domain event.
- `write_meta` — (re)publish the manifest.

The minimal diff is the **pure** `reconcile_entries(desired, observed)
-> Vec<KvOp>`; the `Store` execution is the thin adapter around it.

## Consumer (feature `consumer`, mounted in generic services)

- **`connect_pool(database_url)`** — the TLS-validated `PgPool` for the
  `known_*` projection, built through `br_util_postgres::init_pool` so the
  consumer inherits the platform's secure-by-default DB connection posture.
- **`migrate(pool)`** — creates `known_users`, `known_groups` and the junction
  `known_user_group` (`migrations/0001_known_directory.sql`).
- **`DirectoryProjector`** — the KV→PG projector:
  - `reconcile()` — boot-time: read `identity/_meta`, scan the KV users (and
    groups, only if the manifest declares them), idempotently `upsert` each into
    the `known_*` tables, then **DELETE local rows whose id is no longer in KV**
    (orphan-delete = the PII-deletion guarantee). Returns the `DirectoryMeta` it
    read, so the caller can size its readers.
  - `apply_user` / `remove_user` / `apply_group` / `remove_group` — incremental
    single-entity projection on an event.
- **Denormalized-KV → normalized-PG.** The KV group wire is denormalized — a
  `PublishedGroup { name, member_ids }` keyed by `group_id` in the KV key. The
  projector **recomposes** it into the relational form: `known_groups(group_id,
  name)` plus one `known_user_group(group_id, user_id)` row per `member_id`. A
  group upsert runs in one transaction (upsert the row, replace its junction
  rows) so membership never half-applies. The recompose is the **pure**
  `member_rows(group_id, &group) -> Vec<MemberRow>`.
- **Typed readers carry the id** (recomposed from the KV key, so a caller never
  holds a bare `{ name, member_ids }`): `resolve_user(user_id) ->
  Option<KnownUser>` (`KnownUser` carries `user_id` + `email` + `first_name` +
  `last_name`), `is_member(group_id, user_id) -> bool` (junction lookup),
  `group_name(group_id) -> Option<&str>`.

These readers resolve over `DirectorySnapshot`, the in-memory normalized
projection; the PG-backed readers share its semantics. **Auto-degrade**: a
snapshot built from a manifest that does not declare `groups` returns `None` /
`false` / empty from the group readers — driven by the manifest, never a flag.

**Precondition — the publisher must reconcile before any consumer boots
(Px-before-Cx).** When `identity/_meta` is absent, `reconcile()` treats the
roster as empty and orphan-deletes every local `known_*` row (PII deletion
stays a guarantee). A consumer that boots ahead of identity's first reconcile
therefore flushes its projection; the absent-manifest branch emits
`tracing::warn!` so the mis-ordered boot is observable, but the deploy must
order identity's first reconcile before any reader.

## Tenancy-agnostic (hard rule)

Like `br-core-directory`, the kit names **no** orgs / tenancy concept. It
reads/writes the core contract and the opaque `extensions` bag generically;
`organization_id` is a project extension a tenancy-aware consumer reads on its
own side via `PublishedUser::extension("…")`, never a field this kit knows.

## Tested here vs deferred to WU9 e2e

WU4 ships the kit + **unit tests for the pure logic**, no I/O:

- `reconcile_entries` — empty/unchanged/changed/orphan-delete/mixed diffs.
- `member_rows` — `member_ids` → junction rows, carrying the key-derived
  `group_id`.
- `orphans` — observed ids absent from desired (the projector's delete set).
- `DirectorySnapshot` — `resolve_user` / `is_member` / `group_name`, including
  **auto-degrade** when groups are absent from the manifest.

The KV `Store` adapter and the sqlx projector execution are thin I/O; their
real-PG / real-NATS conformance (the **Px/Cx** suites, incl. orphan-delete and
reconnect-replay end to end) lives in **br-e2e-harness (WU9)**, not here.

## Why

| Thing | Why it is the way it is |
|---|---|
| One crate, two features, `publisher` excludes `br-util-postgres` | The dependency asymmetry is real: the publisher touches KV only; pulling `sqlx` into identity for code it never runs would be dead weight. A feature split keeps one contract crate while honoring the asymmetry. |
| `DirectorySource` is the only seam | The project owns its domain→`Published*` mapping and its source of truth; the kit owns the reconcile/orphan-delete mechanism. Anything project-varying stays behind this trait. |
| The projector re-upserts every desired entry rather than diffing values | The local `known_*` row would have to be re-read to compare; an idempotent `ON CONFLICT … DO UPDATE` over the KV scan is cheaper to read and equally correct. Only deletes need the observed-vs-desired set difference. |
| Group upsert replaces its junction rows in one transaction | Membership is derived from the denormalized `member_ids`; deleting then re-inserting inside one transaction makes a membership change atomic and idempotent under redelivery. |
| Readers resolve over `DirectorySnapshot`, a pure projection | It makes resolution + auto-degrade unit-testable with no I/O; the PG-backed readers mirror the same semantics, proven end to end in WU9. |
| `delete_group` | purges the junction via the `known_user_group` FK `ON DELETE CASCADE` — the contract relies on it |
