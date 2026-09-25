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
`## [<version>]` heading here that matches the `version` in `Chart.yaml`, and a
version bump for every change under `charts/br-common-service/` outside `ci/`.

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
  `replicaCount`, `resources`, `postgres.host`, `nats.url`, and the two DSN
  Secret names fail the render when absent, with a message that names them.
- **The two-role Postgres DSNs** (`DATABASE_URL` as the app role,
  `DATABASE_URL_OWNER` as the owner role, for migrations only), built by
  interpolation from Secret keys `username` / `password`; the principle that
  every role password is alphanumeric is documented at the interpolation site.
  `postgres.trustedNetwork: true` sets `TRUSTED_NETWORK_HOSTS` to exactly
  `postgres.host`; the default is TLS required.
- **Render guards**: the image tag must be a release version inside the
  service chart's `botresources.ai/supported-app-versions` annotation (unless
  `image.enforceSupportedVersions: false`); `replicaCount` above
  `maxReplicas`; a PodDisruptionBudget that leaves no pod evictable; an owner
  Secret equal to the app Secret; a `commonLabels` entry that replaces a
  library label; a probe override that is not a timing field.
- **Hardened pod by default**: non-root UID 65532, `RuntimeDefault` seccomp,
  no privilege escalation, read-only root filesystem, every capability dropped,
  service-account token not mounted.
- **Rollout on Secret change**: the Stakater Reloader annotation lists every
  Secret the pod reads.
