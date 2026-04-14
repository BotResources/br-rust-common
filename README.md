# br-service-core

Generic service infrastructure core for BotResources projects.

Provides the shared foundation that all backend services depend on: authentication (Passport), database (RLS, connection pools), messaging (NATS, KV), configuration, and foundational types (UserId, events).

## Modules

| Module | Purpose |
|--------|---------|
| `passport` | `Passport` enum (Human/Service) with typed identity fields + generic `claims: Value` |
| `passport_header` | `PassportHeader` trait — base64 JSON encode/decode for `X-Passport` header |
| `middleware` | Axum middleware that parses `X-Passport` into `Extension<Passport>` |
| `rls` | `set_rls_context()` — injects `app.current_user_id`, `app.is_super_admin`, `app.is_active` into Postgres session |
| `grant` | `grant_app_access(pool, role_name)` — parameterized GRANT for app role |
| `db` | `init_pool()`, `init_migration_pool()`, `validate_database_tls()` |
| `config` | `Environment` enum, `Config::from_env()` |
| `error` | `InfraError` (Db, Config, Unauthenticated) |
| `ids` | `UserId(Uuid)`, `ServiceAccountId(Uuid)` newtypes |
| `event` | `EventMetadata`, `RawEvent`, `DomainEvent` |
| `kv` | `KvPorts` trait + `InMemoryKv` (behind `test-support` feature) |
| `kv_nats` | `NatsKv` — NATS JetStream KV implementation of `KvPorts` |
| `pat` | PAT / API key generation + SHA-256 hashing |
| `net` | `is_localhost()` with `TRUSTED_HOSTS` support |

## Passport Design

The Passport is a transport DTO — it carries identity through the system without imposing domain-specific types.

```rust
enum Passport {
    Human {
        user_id: Uuid,
        is_super_admin: bool,
        is_active: bool,
        claims: serde_json::Value,  // project-specific: email, role, tenant_id, etc.
    },
    Service {
        service_account_id: Uuid,
        claims: serde_json::Value,
    },
}
```

Typed fields (`user_id`, `is_super_admin`, `is_active`) are universal — used by RLS and subscriptions in every project. Everything else goes in `claims`, which each project fills and reads as needed.

## Usage

Add to your workspace `Cargo.toml`:

```toml
[workspace.dependencies]
br-service-core = { git = "https://github.com/BotResources/br-service-core" }
```

## License

MIT
