# Changelog — chart br-common-service

The library chart of the BotResources Rust services built on the
br-rust-common crates (not on br-service-engine): their shared deployment
topology and their ops contract. Its version line is its own, independent of
the crates' workspace version (`Cargo.toml`, root `CHANGELOG.md`); which crate
versions a chart version serves is stated in each entry below.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the chart adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html):
a major changes the ops contract or removes a value, a minor adds an optional
value, a patch changes neither. `.github/scripts/check-chart.sh` requires a
`## [<version>]` heading here that matches the `version` in `Chart.yaml`, and,
for every change under `charts/br-common-service/` outside `ci/`, a version
greater than the base branch's that is not yet released.

## [1.0.1] - 2026-09-25

For services on br-rust-common `1.x`; the names it renders are constants of
br-rust-common `1.4.0` (the same names every `1.x` release reads). A service
chart on `1.0.0` renders the same manifests on `1.0.1`, byte for byte.

### Changed

- **Every name the chart renders is the code's, and the gate proves it.** Each
  variable the binary reads is a constant of the crate that reads it
  (`br_util_boot::env::{ENVIRONMENT, PORT, DATABASE_URL, NATS_URL}`,
  `br_util_postgres::env::{DATABASE_URL_OWNER, TRUSTED_NETWORK_HOSTS}`), and
  each default probe path a constant of the crate that serves it
  (`br_util_observability::LIVENESS_PATH`,
  `br_util_axum_readiness::READINESS_PATH`). `.github/scripts/check-chart.sh`
  reads them from `ci/ops-contract.json` — what the constants print, pinned by
  a test of `tools/br-ops-contract` — and fails when the chart renders a name
  or a default that differs, renders a variable that is neither a constant nor
  one of the chart's own (the DSN parts `PGUSER`, `PGPASSWORD`, `PGUSER_OWNER`,
  `PGPASSWORD_OWNER`; `POSTGRES_HOST`, `POSTGRES_PORT` of `wait-for-postgres`),
  or leaves a constant unplaced. No expected name is spelled in the gate any
  more.
- **README**: a table *Names* gives every rendered name with the constant that
  owns it (or `chart`), checked by the gate; the values table gains an
  *Owned by* column (its *Owner* column is now *Set by*); a section *Adoption*
  says how and when services move to `br_util_boot::BootEnv` and the probe
  routers — in their next sealed patch — and that charter keeps
  `probes.livenessPath: /health` until it adopts `liveness_router()`.

### Removed

- **The `ALLOW_INSECURE_DATABASE` guard.** No br-rust-common `1.x` crate reads
  that variable (`br-util-postgres` stopped reading it before `1.0.0`), so the
  chart no longer names it: an `extraEnv` entry or a `postgres.appPasswordEnv`
  of that name is an ordinary variable of the service. Only a render that
  failed on `1.0.0` renders now; every render that succeeded is unchanged.

## [1.0.0] - 2026-09-25

### Added

- **First release: ops contract 1**, for services on br-rust-common `1.x`
  (`TRUSTED_NETWORK_HOSTS` is the only trusted-host variable since 1.0.0;
  `br-util-postgres::init_migration_pool` reads `DATABASE_URL_OWNER`). Derived
  from the nine pre-library charts of the BotResources platform services that
  run on these crates.
- **Named templates** `br-common-service.all`, `.deployment`, `.service`,
  `.serviceaccount`, `.pdb`, `.networkpolicy`, and the helpers (name, labels,
  selector labels, image tag).
- **The binary-defined values** a service chart sets once: `serviceName`,
  `image.repository`, `port`, `probes.livenessPath` (default `/livez`),
  `probes.readinessPath` (default `/readyz`), `postgres.database`,
  `postgres.appPasswordEnv`, `postgres.migrate`, `args`, `extraEnv`,
  `maxReplicas`, `strategy`, `commonLabels`.
- **No default for a per-environment value**: `env`, `image.tag`,
  `replicaCount`, `resources`, `postgres.host`, `postgres.port`,
  `postgres.trustedNetwork` (and `postgres.sslMode` when it is false),
  `nats.url`, and the two DSN Secret names (the deploying repository's
  objects) fail the render when absent, with a message that names them.
- **The two-role Postgres DSNs** (`DATABASE_URL` as the app role,
  `DATABASE_URL_OWNER` as the owner role, for migrations only), built by
  interpolation from Secret keys `username` / `password`; the principle that
  every role password is alphanumeric is documented at the interpolation site.
  `postgres.trustedNetwork: true` sets `TRUSTED_NETWORK_HOSTS` to exactly
  `postgres.host`; `false` requires TLS: both DSNs end with
  `?sslmode=<postgres.sslMode>` (`require`, `verify-ca` or `verify-full`),
  the mode `br-util-postgres` requires of a remote host at boot.
- **Render guards**: the image tag must be a release version (SemVer 2.0
  `MAJOR.MINOR.PATCH[-PRERELEASE]`, optional leading `v`, no build metadata,
  which an OCI tag cannot carry) inside the service chart's
  `botresources.ai/supported-app-versions` annotation, optionally followed by
  a digest `@sha256:<64 lowercase hex>` that is admitted whatever the flag says and
  kept in the rendered reference; `image.enforceSupportedVersions: false`
  admits one more kind of tag only, a local build (`local-…`, `local.…`,
  `dev-…`, `dev.…`), while a version tag is always range-checked and `latest`,
  a tag that starts like a version without being one (`0.9`, `0.9.0_x`) or
  that is not a valid OCI tag always fail;
  `replicaCount` above `maxReplicas`; a PodDisruptionBudget that leaves no
  pod evictable, whether its bound is an integer, a quoted integer or a
  percentage (`minAvailable` checked when `replicaCount` is above 0,
  `maxUnavailable` 0 at any count); a `postgres.sslMode` missing off a trusted network, set on one,
  or other than `require` / `verify-ca` / `verify-full`; an owner
  Secret equal to the app Secret; a `commonLabels` entry that replaces a
  library label; a probe override that is not a timing field; an `extraEnv`
  entry (or `postgres.appPasswordEnv`) that would replace a contract variable,
  sets `ALLOW_INSECURE_DATABASE`, repeats a name or has none.
- **Hardened pod by default**: non-root UID 65532, `RuntimeDefault` seccomp,
  no privilege escalation, read-only root filesystem, every capability dropped,
  service-account token not mounted; the default `wait-for-postgres` image is
  `busybox:1.36` pinned to the digest of its multi-arch index
  (`sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662`).
- **Rollout on Secret change**: the Stakater Reloader annotation lists every
  Secret the pod reads.
