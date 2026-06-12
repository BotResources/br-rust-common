CREATE TABLE scope_registry_head (
    id      boolean PRIMARY KEY DEFAULT true CHECK (id),
    version bigint  NOT NULL    DEFAULT 0    CHECK (version >= 0)
);

INSERT INTO scope_registry_head (id, version) VALUES (true, 0);

CREATE TABLE scope_registry_service (
    service_key     text NOT NULL PRIMARY KEY,
    label_key       text NOT NULL,
    description_key text NOT NULL
);

CREATE TABLE scope_registry (
    scope_key       text        NOT NULL PRIMARY KEY,
    owning_service  text        NOT NULL REFERENCES scope_registry_service (service_key),
    label_key       text        NOT NULL,
    description_key text        NOT NULL,
    platform_only   boolean     NOT NULL,
    registered_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at    timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX scope_registry_key_owner_idx
    ON scope_registry (scope_key, owning_service);

CREATE INDEX scope_registry_owning_service_idx ON scope_registry (owning_service);
