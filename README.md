# br-rust-common

Catalogue de petites crates Rust réutilisables par les services BotResources.

## Catalogue

| Crate | Catégorie | Purpose |
|-------|-----------|---------|
| [`br-core-kernel`](crates/br-core-kernel) | core | Typed ID wrappers (`UserId`, `ServiceAccountId`) |
| [`br-core-auth`](crates/br-core-auth) | core | `Passport` DTO + `PassportHeader` trait pour `X-Passport` |
| [`br-core-events`](crates/br-core-events) | core | Types partagés pour événements (`EventMetadata`, `RawEvent`, `DomainEvent`) |
| [`br-util-postgres`](crates/br-util-postgres) | util | Pools Postgres, validation TLS, RLS context, grants |
| [`br-util-axum-auth`](crates/br-util-axum-auth) | util | Middleware Axum qui injecte `Passport` depuis `X-Passport` |

## Règles d'architecture

- `core` = contraintes transverses minimales, aucune dépendance à `util`.
- `util` = wrappers techniques Rust optionnels, peuvent dépendre de `core`.
- Pas de `svc-*` ni de logique métier dans ce repo.
- Chaque crate définit ses propres erreurs — pas d'erreur globale partagée.

## Usage

Chaque crate est publiée par tag git, à piocher à la carte :

```toml
[workspace.dependencies]
br-core-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "br-core-auth-v0.3.0" }
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres-v0.3.0" }
```

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

MSRV : **1.85** (edition 2024).

## License

MIT
