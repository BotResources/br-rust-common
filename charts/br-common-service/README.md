# `br-common-service` — the Rust service topology and its ops contract

`br-common-service` is a Helm **library chart**. It renders nothing on its own.
The application chart of a BotResources Rust service built on the
br-rust-common crates (and not on br-service-engine, whose services use
`br-engine-service`) depends on it and includes its named templates.

It lives in br-rust-common because it encodes what these crates read at boot
— the probe paths, `PORT`, the two-role Postgres DSNs, `TRUSTED_NETWORK_HOSTS`,
`NATS_URL` — and **every one of those names is a constant of the crates**, the
chart gated against it ([Names](#names)): the chart encodes only what the code
owns. Whatever the binary defines lives in the service's chart, in the
repository that builds the binary; whatever differs per environment lives in
the deploying repository, and this library never defaults it — a missing value
fails the render with a message that names it.

- Chart: `oci://ghcr.io/botresources/charts/br-common-service`
- Version line: its own ([`CHANGELOG.md`](CHANGELOG.md)), independent of the
  crates' workspace version. Version 1.x is **ops contract 1**, for services
  on br-rust-common 1.x.

## Use

`Chart.yaml` of the service chart:

```yaml
apiVersion: v2
name: br-svc-example
type: application
version: 0.1.0
appVersion: "1.4.0"
annotations:
  # The service versions this chart can run. Required: the library refuses to
  # render an image.tag outside it.
  botresources.ai/supported-app-versions: ">=1.4.0 <1.5.0"
dependencies:
  - name: br-common-service
    version: "~1.0.0"
    repository: oci://ghcr.io/botresources/charts
```

`templates/service.yaml` — the whole topology:

```yaml
{{ include "br-common-service.all" . }}
```

The named templates read the **top-level** `.Values` of the context they are
given, so the service chart's `values.yaml` holds the library values directly,
not under a `br-common-service:` key. A service chart that needs its own
resource (a bootstrap Job, a second Service) writes it beside the include; one
that needs to compute a value (a Reloader entry, a tag) passes the library a
modified copy of the context.

| Template | Renders |
|---|---|
| `br-common-service.all` | all of the below, the PDB and NetworkPolicy only when enabled |
| `br-common-service.serviceaccount` | `ServiceAccount` |
| `br-common-service.service` | `Service` (ClusterIP, port `http`) |
| `br-common-service.deployment` | `Deployment` |
| `br-common-service.pdb` | `PodDisruptionBudget`, or nothing unless `podDisruptionBudget.enabled` |
| `br-common-service.networkpolicy` | `NetworkPolicy`, or nothing unless `networkPolicy.enabled` |
| `br-common-service.name`, `.labels`, `.selectorLabels`, `.image`, `.imageTag` | helpers for the service chart's own resources (`.imageTag` is the checked `image.tag`, digest included) |

## The ops contract

What the library renders the same way for every service, without a value:

| Contract | Rendered |
|---|---|
| Names | Deployment, Service, ServiceAccount, PDB, NetworkPolicy and container are all `serviceName` — not derived from the release name, because other services and the gateway address the service by its in-namespace Service name. |
| Selector | `app.kubernetes.io/name: <serviceName>`, `app.kubernetes.io/instance: <release name>` — immutable once deployed: render with a stable release name. |
| Labels | the selector, plus `app.kubernetes.io/managed-by`, `app.kubernetes.io/component: backend`, `botresources.ai/env: <env>`, plus `commonLabels` — on every resource and on the pod template. |
| HTTP | container port `http` = Service port `http` = `PORT` = `port`. The port name is part of the contract: the GraphQL gateway builds subgraph URLs from it. |
| Probes | readiness `GET <readinessPath>` (5 s delay, every 5 s); liveness `GET <livenessPath>` (30 s delay, every 10 s); optional startup `GET <livenessPath>` (every 5 s, 30 failures). Readiness never backs liveness: a service may stay unready for long (until its scope declaration is accepted) and must not be restarted for it. |
| `ENVIRONMENT` | `env`. |
| `DATABASE_URL` | `postgres://$(PGUSER):$(PGPASSWORD)@<host>:<port>/<database>`, with `PGUSER` / `PGPASSWORD` from keys `username` / `password` of `postgres.appSecretName` — the least-privilege runtime role. Off a trusted network it ends with `?sslmode=<postgres.sslMode>`. |
| `DATABASE_URL_OWNER` | the same shape as `PGUSER_OWNER` / `PGPASSWORD_OWNER` from `postgres.ownerSecretName` — the owner role, read by `br_util_postgres::init_migration_pool` at boot to migrate, then closed. It bypasses row-level security and never backs a request. Absent when `postgres.migrate` is false. |
| `TRUSTED_NETWORK_HOSTS` | exactly `postgres.host`, when `postgres.trustedNetwork` is true, and neither DSN carries an `sslmode`. When it is false, the variable is absent and both DSNs end with `?sslmode=<postgres.sslMode>`: `br-util-postgres` refuses, at boot, a remote DSN whose `sslmode` is not `require`, `verify-ca` or `verify-full`. |
| `NATS_URL` | `nats.url`. |
| Contract variables | the variables above are the library's. Kubernetes keeps the last of two entries with the same name, so `extraEnv` may not redeclare one, nor `postgres.appPasswordEnv`, nor one of its own entries: the render fails. An extra `DATABASE_URL: $(DATABASE_URL_OWNER)` would otherwise run every request as the owner role, which bypasses row-level security. |
| Boot order | init container `wait-for-postgres` waits until `postgres.host:port` accepts TCP; the service then migrates, binds NATS and serves. |
| Rollout | annotation `secret.reloader.stakater.com/reload` lists every Secret the pod reads (owner, app, `extraEnv` and `extraInitContainers` references, pull secrets): the DSNs are read once at boot, so a rotated password needs a new pod. |
| Hardening | non-root UID/GID 65532, `RuntimeDefault` seccomp, no privilege escalation, read-only root filesystem, all capabilities dropped, service-account token not mounted. |

### Role passwords must be alphanumeric

Both DSNs are built by **raw string interpolation**. A password that contains
a URL-reserved character (`/ + = @ : # ? %`) corrupts the URL, and the driver
fails with `password authentication failed` although the Secret and the role
agree. Generate every role password from an alphanumeric charset
(`tr -dc 'A-Za-z0-9' </dev/urandom | head -c 32`), never from base64.

## Names

Every name the library renders for the binary is a constant of the crate that
reads it, and every default probe path a constant of the crate that serves it:
the chart renders no variable the binary does not read, and probes no default
path the binary does not serve. A chart
cannot import a Rust constant, so the link is a gate
([CI](#ci)): `tools/br-ops-contract` prints the constants into
[`ci/ops-contract.json`](ci/ops-contract.json) (its test fails when that file
is stale), and `check-chart.sh` fails when the chart renders a name or a
default that differs from it, renders a variable that is neither a constant
nor listed below as the chart's own, or leaves a constant unplaced — and when
this table does not name the owner of each.

**Owned by** is where a name is defined: a constant (`crate::CONSTANT`), or
`chart` for the few names only the chart uses.

| Name | Kind | Owned by | Rendered from |
|---|---|---|---|
| `ENVIRONMENT` | variable | `br_util_boot::env::ENVIRONMENT` | `env` |
| `PORT` | variable | `br_util_boot::env::PORT` | `port` |
| `DATABASE_URL` | variable | `br_util_boot::env::DATABASE_URL` | the app DSN: `postgres.appSecretName`, `postgres.host`, `postgres.port`, `postgres.database`, `postgres.sslMode` |
| `NATS_URL` | variable | `br_util_boot::env::NATS_URL` | `nats.url` |
| `DATABASE_URL_OWNER` | variable | `br_util_postgres::env::DATABASE_URL_OWNER` | the owner DSN, when `postgres.migrate`: `postgres.ownerSecretName` and the same host, port, database and mode |
| `TRUSTED_NETWORK_HOSTS` | variable | `br_util_postgres::env::TRUSTED_NETWORK_HOSTS` | `postgres.host`, when `postgres.trustedNetwork` |
| `/livez` | liveness and startup probe path, default | `br_util_observability::LIVENESS_PATH` | `probes.livenessPath` |
| `/readyz` | readiness probe path, default | `br_util_axum_readiness::READINESS_PATH` | `probes.readinessPath` |
| `HOST` | variable, not rendered | `br_util_boot::env::HOST` | — the binary's default, every IPv4 interface (`0.0.0.0`), is what a pod needs for its probes |
| `/metrics` | metrics path, not rendered | `br_util_observability::METRICS_PATH` | — the library configures no metrics scrape |
| `PGUSER` | variable | chart | key `username` of `postgres.appSecretName`, interpolated into `DATABASE_URL` |
| `PGPASSWORD` | variable | chart | key `password` of `postgres.appSecretName`, interpolated into `DATABASE_URL` |
| `PGUSER_OWNER` | variable | chart | key `username` of `postgres.ownerSecretName`, interpolated into `DATABASE_URL_OWNER` |
| `PGPASSWORD_OWNER` | variable | chart | key `password` of `postgres.ownerSecretName`, interpolated into `DATABASE_URL_OWNER` |
| `POSTGRES_HOST` | variable of `wait-for-postgres` | chart | `postgres.host` |
| `POSTGRES_PORT` | variable of `wait-for-postgres` | chart | `postgres.port` |

`postgres.appPasswordEnv` and `extraEnv` name variables of the service binary
itself: the service chart owns those names, not this library.

## Values

**Set by** says who sets a value. *Service chart*: what the binary defines, the
same in every environment — set once, in the repository that builds the
binary. *Environment*: what differs per environment, and the names of the
deploying repository's own objects — set by the deploying repository.
**Owned by** says what defines the name the value is rendered under, or its
default ([Names](#names)): the br-rust-common constant; `chart` when only the
chart uses it (topology, scheduling, the DSN shape); `service chart` when the
value names a variable of the service binary itself.
**Required** values have no default and fail the render when absent.

| Value | Set by | Owned by | Status | Meaning |
|---|---|---|---|---|
| `serviceName` | service chart | chart | **required** | DNS-1035 name of every resource and of the selector. |
| `image.repository` | service chart | chart | **required** | Image without tag. |
| `image.tag` | environment | chart | **required** | The version to run (Kargo writes it on promotion): a release version — SemVer 2.0 `MAJOR.MINOR.PATCH[-PRERELEASE]`, optional leading `v`, no build metadata (`+…` is not valid in an OCI tag) — inside the service chart's `botresources.ai/supported-app-versions` annotation. A pre-release is inside the range only if the range names a pre-release, e.g. `>=0.5.0-0 <0.6.0` (Masterminds/Kargo semantics). It may end with a digest, `@sha256:<64 lowercase hex>`, rendered as `<repository>:<tag>@sha256:…`: the digest is admitted whatever `image.enforceSupportedVersions` says, and the tag before it is checked as if it were alone. Anything else fails the render: `latest`, a tag that starts like a version without being a full one (`0.9`, `0.9.0_x`), a tag that is not a valid OCI tag. |
| `image.enforceSupportedVersions` | local build only | chart | `true` | `false` admits one more kind of tag: a local build, whose tag starts with `local-`, `local.`, `dev-` or `dev.` (e.g. `local-build`, `dev-<sha>`) and has no version to check. Nothing else: a version tag is still checked against the range (no values file can pair the chart with a binary outside it), and `latest` or a tag that starts like a version without being one still fails. |
| `image.pullPolicy` | service chart | chart | `IfNotPresent` | |
| `port` | service chart | `br_util_boot::env::PORT` | **required** | `PORT`, the container port and the Service port. |
| `args` | service chart | chart | none | Container arguments, when the image needs a subcommand. |
| `probes.livenessPath` | service chart | `br_util_observability::LIVENESS_PATH` | `/livez` | Liveness and startup path, served by `br_util_observability::liveness_router`. Set it only for a binary that serves liveness elsewhere, until it adopts the router ([Adoption](#adoption)). |
| `probes.readinessPath` | service chart | `br_util_axum_readiness::READINESS_PATH` | `/readyz` | Readiness path, served by `br_util_axum_readiness::readiness_router`. |
| `probes.readiness`, `probes.liveness` | service chart | chart | 5/5 s, 30/10 s | Timing only: `initialDelaySeconds`, `periodSeconds`, `timeoutSeconds`, `successThreshold`, `failureThreshold`. Any other key fails. |
| `probes.startup.enabled` (+ timing) | service chart | chart | `false` | Startup probe on the liveness path, 5 s × 30. |
| `maxReplicas` | service chart | chart | none | The most pods the binary is safe with (1 for a service with an unelected in-process scheduler); `replicaCount` above it fails. |
| `strategy` | service chart | chart | Kubernetes default | Deployment strategy, e.g. `{type: Recreate}` for a single-pod service. |
| `postgres.database` | service chart | chart | **required** | The database the service owns. |
| `postgres.migrate` | service chart | `br_util_postgres::env::DATABASE_URL_OWNER` | `true` | `false` for a service that never migrates (a restricted writer on another service's database): no owner DSN. |
| `postgres.appPasswordEnv` | service chart | service chart | none | Name of an extra variable carrying the app role's password, for a binary that provisions that role at boot. Never a contract variable (refused). |
| `postgres.appSecretName` | environment | `br_util_boot::env::DATABASE_URL` | **required** | Secret of the runtime role (`username`, `password`). Never the owner Secret (refused). |
| `postgres.ownerSecretName` | environment | `br_util_postgres::env::DATABASE_URL_OWNER` | **required** unless `migrate: false` | Secret of the owner role (`username`, `password`). |
| `postgres.host` | environment | chart | **required** | The Postgres read-write Service. |
| `postgres.port` | environment | chart | **required** | The port of `postgres.host` (5432 for a CNPG `<cluster>-rw` Service). |
| `postgres.trustedNetwork` | environment | `br_util_postgres::env::TRUSTED_NETWORK_HOSTS` | **required** | `true` when `postgres.host` speaks plaintext on a trusted network (an in-namespace Service without TLS), `false` to require TLS (with `postgres.sslMode`). No default: either one would be wrong at runtime somewhere while the render succeeds — `false` crash-loops the pod on a plaintext Service, `true` sends passwords in clear to a host that should speak TLS. |
| `postgres.sslMode` | environment | chart | **required** when `trustedNetwork` is false, refused when it is true | `require`, `verify-ca` or `verify-full`, appended to both DSNs as `?sslmode=<mode>`. `require` encrypts without checking the server certificate; `verify-ca` / `verify-full` check it against the public web roots the binary's TLS stack carries (sqlx with webpki roots), so they fit a server whose certificate a public CA signed. |
| `nats.url` | environment | `br_util_boot::env::NATS_URL` | **required** | `NATS_URL`. |
| `env` | environment | `br_util_boot::env::ENVIRONMENT` | **required** | `ENVIRONMENT` and the label `botresources.ai/env`. |
| `replicaCount` | environment | chart | **required** | Pods; bounded by `maxReplicas`. |
| `resources` | environment | chart | **required** | Requests and limits of the service container. |
| `imagePullSecrets` | environment | chart | none | Kubernetes shape: `[{name: …}]`. |
| `nodeSelector`, `tolerations`, `affinity` | environment | chart | none | Scheduling. |
| `topologySpreadEnabled` | environment | chart | `false` | Spread pods across nodes (`ScheduleAnyway`). |
| `podDisruptionBudget.enabled` | environment | chart | `false` | With exactly one of `minAvailable` / `maxUnavailable`, each an integer or a percentage (`"50%"`, 0–100); a quoted integer is read as the integer. A budget that leaves no pod evictable fails: it would hang every node drain — `minAvailable` ≥ `replicaCount` when `replicaCount` is above 0, a percentage counted as Kubernetes counts it (rounded up: `100%`, or `67%` of 3 pods), or `maxUnavailable` `0` / `0%` at any `replicaCount`. At `replicaCount` 0 there is no pod to evict, so any `minAvailable` renders: an environment scaled to 0 may keep its budget. |
| `networkPolicy.enabled` | environment | chart | `false` | With `networkPolicy.ingress` and/or `networkPolicy.egress` (rules, verbatim); each key present adds its direction to `policyTypes`. Named `serviceName`. |
| `commonLabels` | service chart | chart | none | String labels added to every resource and the pod template, e.g. `graphql-federation/component: subgraph` for gateway discovery. The five library labels cannot be replaced. |
| `extraEnv` | service chart | service chart | none | Variables the binary reads beyond the contract, appended last (so they may reference `$(DATABASE_URL)` and the others). A contract variable, the `postgres.appPasswordEnv` name, a repeated name or an entry without a name fails the render. |
| `extraInitContainers` | service chart | chart | none | Appended after `wait-for-postgres`; one without a `securityContext` gets the hardened container defaults. Its environment is its own (not checked against the contract variables): a wait container may read the owner DSN. |
| `waitForPostgres.enabled`, `.image` | service chart | chart | `true`, `busybox:1.36@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662` | The TCP wait before boot. The default is busybox 1.36 pinned to the digest of its multi-arch index (amd64 and arm64 among others), so the tag cannot move under a running environment. |
| `automountServiceAccountToken` | service chart | chart | `false` | `true` only if the binary calls the Kubernetes API. |
| `podSecurityContext`, `containerSecurityContext` | service chart | chart | hardened | A key set here replaces the default key of the same name, a key set to `null` removes it. |

The deploying repository's own objects — the DSN Secrets, the pull secrets,
the Postgres and NATS Services — are environment values even when every
environment names them alike: the service chart, in the repository that
builds the binary, never names them.

## Adoption

The names above are the ones every service on these crates already reads and
serves, so the chart renders exactly what it rendered before the constants
existed. What changes is where a service takes them from: from br-rust-common
1.4.0 on, a binary should never spell a contract name itself.

| Instead of | A service uses |
|---|---|
| reading `ENVIRONMENT`, `PORT`, `HOST`, `DATABASE_URL`, `NATS_URL` by hand | `br_util_boot::BootEnv::from_env()`, which reads, types and validates all of them and reports every problem at once |
| `.route("/livez", liveness_route())` | `.merge(br_util_observability::liveness_router())` |
| `.route("/readyz", readiness_route(handle))` | `.merge(br_util_axum_readiness::readiness_router(handle))` |
| `.route("/metrics", metrics_route(handle))` | `.merge(br_util_observability::metrics_router(handle))` |

**A service adopts them in its next sealed patch, not before.** The Services
registry's release gate forbids a service code change without a sealed patch
that has not been implemented yet, so no service is changed for this alone:
each moves to `BootEnv` and the routers when it next ships a patch, and until
then keeps reading the same names by hand — nothing differs at runtime.

**A service that serves a probe elsewhere keeps its override until it
adopts.** charter serves liveness on `/health`, not on `/livez`: its chart
keeps `probes.livenessPath: /health` set explicitly (as
[`ci/example-service`](ci/example-service/values.yaml) does), and drops the
override in the patch that mounts `liveness_router()`, from which on the
default — `br_util_observability::LIVENESS_PATH` — is right.

What `BootEnv` asks of a binary that read these variables with its own
defaults:

- `ENVIRONMENT` and `PORT` are **required**: the chart always renders them, so
  a default in the binary could only hide a chart that forgot one. A local run
  or a test harness that spawns the binary sets them.
- `ENVIRONMENT` is exactly `local`, `dev`, `test`, `uat` or `prod`
  (`br_util_boot::Environment`) — the lowercase spelling the chart's
  `botresources.ai/env` label carries, no other case, no alias.
- `HOST` is an IP address (`0.0.0.0` when unset), not a host name.
- `NATS_URL` states its scheme (`nats://`, `tls://`, `ws://`, `wss://`).
- `BootEnv` never loads a `.env` file (setting process variables is unsound
  once threads run): a binary that wants one loads it before its runtime
  starts, or reads it into a map and calls `BootEnv::from_lookup`.

## Moving a pre-library chart onto the library

[`ci/diff-against-chart.sh`](ci/diff-against-chart.sh) renders a pre-library
chart and the example chart with the same per-environment values and prints
the normalised diff, environment by environment. Run it on each chart before
its move.

**charter — verified.** [`ci/example-service`](ci/example-service) mirrors
the pre-library chart of charter. Rendered with the deploying repository's
dev, uat and prod values, the two charts differ in exactly four ways:

- `app.kubernetes.io/managed-by` is `Helm` (`.Release.Service`), was `helm`;
- the pod does not mount a service-account token;
- `extraEnv` entries (`SCOPE_DECLARATION_ENABLED`) come after the contract
  variables, no longer before them;
- the `wait-for-postgres` image is the same `busybox:1.36`, pinned by digest.

The deploying repository adds to each environment's values what the
pre-library chart defaulted in its own `values.yaml`: `postgres.port`,
`postgres.trustedNetwork: true` (the pre-library chart hard-coded
`TRUSTED_NETWORK_HOSTS`) and the two DSN Secret names — see
[`ci/example-service/values-deploy-additions.yaml`](ci/example-service/values-deploy-additions.yaml).

**The other eight — read, not rendered.** Like charter, each labels
`managed-by: helm` and mounts the service-account token, and each but ux
(which waits with `psql`, below) waits on the unpinned `busybox:1.36`. Their
templates show these further differences, none of them verified by a render
yet:

| Charts | Pre-library | On the library |
|---|---|---|
| identity, projects, services, tasks, timesheet, ux | the owner credentials and `DATABASE_URL_OWNER` come before the app block | the app block comes first; harmless, each DSN still follows its own credentials |
| identity, projects, services, tasks, timesheet, ux | `<SERVICE>_APP_PASSWORD` from the app Secret, before `DATABASE_URL` | `postgres.appPasswordEnv` (same place) |
| website | `WEBSITE_APP_PASSWORD` after `DATABASE_URL_OWNER` | `postgres.appPasswordEnv`, before `DATABASE_URL`; harmless, no DSN reads it |
| website | `WEBSITE_ANON_WRITER_PASSWORD`, from another Secret | `extraEnv` |
| identity | `IDENTITY_PASSPORT_PASSWORD`, from a third Secret; `BEARER_SEAL_KEY` | `extraEnv` |
| identity, website, anonymous-writer, services | the five `S3_*` object-store variables | `extraEnv` |
| identity | `args: ["serve"]`; the bootstrap Job | `args`; the Job stays in the service chart, beside the include |
| ux | a bounded `psql` wait (owner DSN, ownership check) instead of the `nc -z` wait | `waitForPostgres.enabled: false` plus `extraInitContainers` |
| tasks, timesheet, ux | a Deployment `strategy`; a startup probe | `strategy`; `probes.startup` (compare the timing) |
| website, anonymous-writer | a NetworkPolicy | `networkPolicy` (compare the rules) |
| anonymous-writer | no owner role | `postgres.migrate: false` |
| projects, services, timesheet | `SCOPE_DECLARATION_ENABLED` before the contract variables | `extraEnv`, after them |

## CI

[`.github/scripts/check-chart.sh`](../../.github/scripts/check-chart.sh), job
`chart` of `ci.yml`: lint, the [names](#names) against
[`ci/ops-contract.json`](ci/ops-contract.json) (each rendered name and default
path equal to its constant, no variable beyond them but the chart's own, no
constant unplaced, each documented with its owner), per-environment render of
[`ci/example-service`](ci/example-service) with field-by-field assertions of
the contract — every expected name read from the contract file, never
spelled — a render with every optional field, one failing render per guard,
and the version gate (a change outside `ci/` needs a version greater than the
base's, not yet tagged, and a `CHANGELOG.md` entry).
`ci/ops-contract.json` is what the crates' constants print
([`tools/br-ops-contract`](../../tools/br-ops-contract/src/main.rs)); the test
of that tool, in the `cargo test` job, fails when the file is stale —
regenerate it with
`cargo run -q -p br-ops-contract > charts/br-common-service/ci/ops-contract.json`.
The chart gate therefore needs no Rust toolchain, and neither does the release
workflow that re-runs it.
[`.github/workflows/chart-release.yml`](../../.github/workflows/chart-release.yml)
publishes a new version from `main` or `release/**`, once: a published version
and its tag `chart/br-common-service/v<version>` are never replaced, and a run
for a version already published fails unless the published package equals
the commit's. The release decisions — the version gate, the tag and registry
lookups, the publish table — are functions of
[`chart-release-lib.sh`](../../.github/scripts/chart-release-lib.sh), tested
case by case by
[`test-chart-release.sh`](../../.github/scripts/test-chart-release.sh) in the
same job.
