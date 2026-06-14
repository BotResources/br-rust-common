# br-core-directory

Frozen **read** contract for the identity **Published Language**: the typed
shapes a service deserializes when it reads the identity roster from NATS KV —
"who is user X", "is X in group Y", "name of group Z". Tier `core` — pure serde
DTOs, KV-key conventions and a manifest type; **no I/O, no transport, no
`async`, no `br-util-*` / `sqlx` / `async-nats` dependency** so it imports
cleanly as a wire oracle.

`br-core-directory` implements the *identity Published Language* read contract.
("Directory" is the crate name because `br-identity-{domain,app}` in this lib is
the **scope registry**; this read/enumeration contract over users + groups is a
*directory*. The doctrine/Outline vocabulary "identity Published Language" maps
to this crate.)

**Purpose.** The identity bounded context is the **only writer** of this KV;
every other service is a **reader** (services are trusted under the
network-isolation model — no extra trust boundary). The PL is **directory /
display / enumeration**, never authZ: effective permissions stay the `scopes`
Passport claim, group-resolved inside identity and rebuilt fresh per request — a
denormalized, staleable permission copy in KV would be dead, write-only data.

**Why this is a freeze, not an invention.** The wire is **extracted from the
live, already-consumed Published Language in be-botresources** (`svc-identity`
publishes it, downstream services read it today). The wire — field names, KV key
layout, JSON casing — is frozen as that reference has it; a Go anchor mirrors
this crate's serde shape and the e2e-harness imports this crate as the oracle to
deserialize the Go-frozen wire (lib drift → the deser fails).

## The contract surface

- **`PublishedUser`** — typed **core/kernel** fields `email`, `first_name`,
  `last_name`; everything else rides in the flattened `extensions` bag. The
  `user_id` is **not** a body field — it is the KV key suffix.
- **`PublishedGroup`** — typed **core/kernel** fields `name`, `member_ids`
  (member presence is derivable: `has_member(user_id)`); the rest rides in
  `extensions`. The `group_id` is the KV key suffix, not a body field.
- **`DirectoryMeta`** (`identity/_meta`) — declares the published `entities`
  (`users` [+ `groups`]) and a `version`. Consumers self-configure from it and
  **auto-degrade** — no `groups` declared ⇒ later group readers return empty. The
  manifest is **inferred from the source**, never a deploy flag.
- **KV keys, frozen** — `identity/users/{id}`, `identity/groups/{id}`,
  `identity/_meta`, exposed as `USERS_KEY_PREFIX` / `GROUPS_KEY_PREFIX` /
  `META_KEY` and the `user_kv_key` / `group_kv_key` builders + their reverse
  `*_id_from_kv_key` parsers.

## Core + extension (the kernel binds, the project rides alongside)

Like the Passport `claims` bag: a **generic** service binds the **core** typed
fields only; a project's extra fields are carried in `extensions`
(`#[serde(flatten)]` map) and read only by the consumers that care, via
`.extension("key")`. The kernel is exactly the **project-invariant** facts;
anything that varies between projects stays an extension. The bag is opaque —
the core contract never names a project field; a consumer reads what it needs
entirely on its own side.

- be-botresources fields that ride as extensions today: a user's `version`,
  `avatar_object_key`, `avatar_mime`, `locale`, `disabled_at`; a group's
  `version`, `is_system`.
- **Promotion rule:** a generic service needing an extension field **that is
  project-invariant** ⇒ promote it to core. A field that varies between projects
  stays an extension.

## Out of scope

The publisher kit (reconcile / orphan-delete / incremental publish) and the
consumer kit (`known_*` migration, KV→PG projector, typed readers) are
`br-util-directory` — they carry I/O and depend on this crate. PII deletion is
the publisher/consumer kit's reconcile guarantee, not this crate's. The Go wire
anchor and the Px/Cx conformance suites live in `br-e2e-harness`.

## Why

| Thing | Why it is the way it is |
|---|---|
| Core fields are `first_name` / `last_name`, not a single `name` | The frozen live wire splits the name; a single `name` core field could not deserialize a real be-botresources KV value, breaking the freeze. |
| Project fields ride in `extensions`, though the live wire has some inline on the group | The typed core must work across projects; flatten keeps the wire byte-identical while the core stays a project-invariant kernel that names no project field. |
| Round-trip tests assert `serde_json::Value` equality, not byte equality | `#[serde(flatten)]` re-emits typed fields before bag fields, so byte order differs from the live producer; semantic (Value) equality is the correct freeze invariant for a JSON KV wire. |
| `PublishedEntity` has an `Other(String)` variant and hand-written serde, not `#[serde(other)]` | A future identity may publish a new entity; an old consumer must auto-degrade, not crash — so an unknown value is captured (not dropped) and round-trips, while adding a known variant still forces every match to be revisited. |
| `_meta` is shipped here though it is not yet live in be-botresources | It is the designed auto-degrade manifest the consumer kit (WU4) and the P-suite conformance need frozen now; freezing the shape early is the point of the pre-freeze normalization. |
