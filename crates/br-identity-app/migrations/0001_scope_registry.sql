-- Scope-registration slice of the Identity bounded context.
--
-- The domain singleton `ScopeRegistry` is persisted as current-state rows (this
-- is the state-stored model — the emitted events are deltas/notifications, NOT
-- the rehydration mechanism). Three tables:
--
--   scope_registry_head    — the singleton aggregate's optimistic-lock version
--   scope_registry_service — one row per registered service (its manifest)
--   scope_registry         — one row per owned scope; UNIQUE(scope_key) is the
--                            final net behind the aggregate's uniqueness invariant
--
-- No row carries a per-user ownership dimension, so there is NO row-level
-- security here: the registry is scope-gated platform data (every authenticated
-- caller in the platform reads the same registry), not per-tenant rows. RLS
-- would add policy maintenance cost with no isolation to gain — the correct
-- engineering call is to skip it (see the platform RLS doctrine).
--
-- This migration is run by an explicit `migrate(pool)` the composing service
-- calls on boot through its owner/migration pool — NOT auto-provisioning: the
-- objects are declared here and applied by the operator, never created
-- on-demand at request time.

-- The singleton aggregate's version. Exactly one row, pinned by a CHECK so a
-- second row can never be inserted. `version` is the optimistic-lock token: a
-- state-changing command bumps it once; the save path conditions its UPDATE on
-- the loaded version and treats a zero-row update as a concurrency conflict.
CREATE TABLE scope_registry_head (
    id      boolean PRIMARY KEY DEFAULT true CHECK (id),
    version bigint  NOT NULL    DEFAULT 0    CHECK (version >= 0)
);

-- Seed the single head row at version 0 (the empty registry). Idempotent across
-- a re-run because migrations run once, and the CHECK forbids a second row.
INSERT INTO scope_registry_head (id, version) VALUES (true, 0);

-- One row per registered service: the manifest (key + i18n display keys).
CREATE TABLE scope_registry_service (
    service_key     text NOT NULL PRIMARY KEY,
    label_key       text NOT NULL,
    description_key text NOT NULL
);

-- One row per owned scope. `scope_key` is globally UNIQUE — the database-level
-- guarantee of the aggregate's "a scope key is owned by at most one service"
-- invariant. A future entitlements table can FK `org_entitlements.scope`
-- onto `scope_registry(scope_key)` (the entitlements table itself is out of
-- scope for this slice). `owning_service` references the service that declared
-- it; the `{service}` segment of `scope_key` always equals `owning_service`
-- (the domain enforces it), and hydration re-validates the consistency.
--
-- registered_at / last_seen_at are RESERVED for a future lifecycle feature
-- (orphan detection); no lifecycle mechanism ships now. Semantics implemented
-- today:
--   - registered_at is set once, on first insert of the row (the scope's first
--     acceptance), and never changed afterwards;
--   - last_seen_at is touched (set to now()) on every (re-)declaration that
--     references the scope — including an idempotent re-declare of an already
--     owned scope — so it always records the most recent time the owning
--     service re-asserted the scope.
CREATE TABLE scope_registry (
    scope_key       text        NOT NULL PRIMARY KEY,
    owning_service  text        NOT NULL REFERENCES scope_registry_service (service_key),
    label_key       text        NOT NULL,
    description_key text        NOT NULL,
    platform_only   boolean     NOT NULL,
    registered_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at    timestamptz NOT NULL DEFAULT now()
);

-- The PRIMARY KEY on scope_key is the global uniqueness net (a key is owned by
-- at most one service) and the FK target a later entitlements migration
-- references.
--
-- This second unique index on (scope_key, owning_service) is the arbiter the
-- save path's upsert names in `ON CONFLICT (scope_key, owning_service)`. The
-- distinction is load-bearing:
--   - a SAME-owner re-declare conflicts on THIS arbiter → the upsert's
--     DO UPDATE fires (idempotent touch of last_seen_at);
--   - a CROSS-owner declare (the racing/contended case) conflicts on the
--     scope_key PRIMARY KEY but NOT on this arbiter → Postgres raises a unique
--     violation (SQLSTATE 23505) because the conflict is on a constraint the
--     ON CONFLICT clause did not name. That raised violation is the final net:
--     the save path classifies it into a terminal rejection, never a nak.
-- Because scope_key is already unique, (scope_key, owning_service) is trivially
-- unique too — this index adds the arbiter, not a new constraint surface.
CREATE UNIQUE INDEX scope_registry_key_owner_idx
    ON scope_registry (scope_key, owning_service);

-- Lookup scopes by their owning service (the grant-admin read groups by
-- service; hydration loads all rows but a per-service read benefits).
CREATE INDEX scope_registry_owning_service_idx ON scope_registry (owning_service);
