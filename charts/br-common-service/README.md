# `br-common-service` — the Rust service topology and its ops contract

`br-common-service` is a Helm **library chart**. It renders nothing on its own.
The application chart of a BotResources Rust service built on the
br-rust-common crates (and not on br-service-engine, whose services use
`br-engine-service`) depends on it and includes its named templates.

It lives in br-rust-common because it encodes what these crates and the
services on them read at boot: the probe paths, `PORT`, the two-role Postgres
DSNs, `TRUSTED_NETWORK_HOSTS`, `NATS_URL`. Whatever the binary defines lives
in the service's chart, in the repository that builds the binary; whatever
differs per environment lives in the deploying repository, and this library
never defaults it — a missing value fails the render with a message that names
it.

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
| `br-common-service.name`, `.labels`, `.selectorLabels`, `.image`, `.imageTag` | helpers for the service chart's own resources |

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
| `DATABASE_URL` | `postgres://$(PGUSER):$(PGPASSWORD)@<host>:<port>/<database>`, with `PGUSER` / `PGPASSWORD` from keys `username` / `password` of `postgres.appSecretName` — the least-privilege runtime role. |
| `DATABASE_URL_OWNER` | the same shape as `PGUSER_OWNER` / `PGPASSWORD_OWNER` from `postgres.ownerSecretName` — the owner role, read by `br_util_postgres::init_migration_pool` at boot to migrate, then closed. It bypasses row-level security and never backs a request. Absent when `postgres.migrate` is false. |
| `TRUSTED_NETWORK_HOSTS` | exactly `postgres.host`, when `postgres.trustedNetwork` is true; absent otherwise, and `br-util-postgres` then refuses a remote DSN without `sslmode=require`. |
| `NATS_URL` | `nats.url`. |
| Boot order | init container `wait-for-postgres` waits until `postgres.host:port` accepts TCP; the service then migrates, binds NATS and serves. |
| Rollout | annotation `secret.reloader.stakater.com/reload` lists every Secret the pod reads (owner, app, `extraEnv` and `extraInitContainers` references, pull secrets): the DSNs are read once at boot, so a rotated password needs a new pod. |
| Hardening | non-root UID/GID 65532, `RuntimeDefault` seccomp, no privilege escalation, read-only root filesystem, all capabilities dropped, service-account token not mounted. |

### Role passwords must be alphanumeric

Both DSNs are built by **raw string interpolation**. A password that contains
a URL-reserved character (`/ + = @ : # ? %`) corrupts the URL, and the driver
fails with `password authentication failed` although the Secret and the role
agree. Generate every role password from an alphanumeric charset
(`tr -dc 'A-Za-z0-9' </dev/urandom | head -c 32`), never from base64.

## Values

**Owner** says who sets a value. *Service chart*: what the binary defines, the
same in every environment — set once, in the repository that builds the
binary. *Environment*: what differs per environment — set by the deploying
repository. **Required** values have no default and fail the render when
absent.

| Value | Owner | Status | Meaning |
|---|---|---|---|
| `serviceName` | service chart | **required** | DNS-1035 name of every resource and of the selector. |
| `image.repository` | service chart | **required** | Image without tag. |
| `image.tag` | environment | **required** | The version to run (Kargo writes it on promotion). Must be a release version inside the service chart's `botresources.ai/supported-app-versions` annotation. |
| `image.enforceSupportedVersions` | local build only | `true` | `false` skips the range check, for a local image whose tag is not a release version. Never in a promoted environment. |
| `image.pullPolicy` | service chart | `IfNotPresent` | |
| `port` | service chart | **required** | `PORT`, the container port and the Service port. |
| `args` | service chart | none | Container arguments, when the image needs a subcommand. |
| `probes.livenessPath` | service chart | `/livez` | Liveness and startup path (`br-util-observability::liveness_route`). |
| `probes.readinessPath` | service chart | `/readyz` | Readiness path (`br-util-axum-readiness`). |
| `probes.readiness`, `probes.liveness` | service chart | 5/5 s, 30/10 s | Timing only: `initialDelaySeconds`, `periodSeconds`, `timeoutSeconds`, `successThreshold`, `failureThreshold`. Any other key fails. |
| `probes.startup.enabled` (+ timing) | service chart | `false` | Startup probe on the liveness path, 5 s × 30. |
| `maxReplicas` | service chart | none | The most pods the binary is safe with (1 for a service with an unelected in-process scheduler); `replicaCount` above it fails. |
| `strategy` | service chart | Kubernetes default | Deployment strategy, e.g. `{type: Recreate}` for a single-pod service. |
| `postgres.database` | service chart | **required** | The database the service owns. |
| `postgres.migrate` | service chart | `true` | `false` for a service that never migrates (a restricted writer on another service's database): no owner DSN. |
| `postgres.appPasswordEnv` | service chart | none | Name of an extra variable carrying the app role's password, for a binary that provisions that role at boot. |
| `postgres.appSecretName` | deploying repository's object | **required** | Secret of the runtime role (`username`, `password`). Never the owner Secret (refused). |
| `postgres.ownerSecretName` | deploying repository's object | **required** unless `migrate: false` | Secret of the owner role (`username`, `password`). |
| `postgres.host` | environment | **required** | The Postgres read-write Service. |
| `postgres.port` | environment | `5432` | |
| `postgres.trustedNetwork` | environment | `false` | `true` when `postgres.host` speaks plaintext on a trusted network (an in-namespace Service without TLS). |
| `nats.url` | environment | **required** | `NATS_URL`. |
| `env` | environment | **required** | `ENVIRONMENT` and the label `botresources.ai/env`. |
| `replicaCount` | environment | **required** | Pods; bounded by `maxReplicas`. |
| `resources` | environment | **required** | Requests and limits of the service container. |
| `imagePullSecrets` | environment | none | Kubernetes shape: `[{name: …}]`. |
| `nodeSelector`, `tolerations`, `affinity` | environment | none | Scheduling. |
| `topologySpreadEnabled` | environment | `false` | Spread pods across nodes (`ScheduleAnyway`). |
| `podDisruptionBudget.enabled` | environment | `false` | With exactly one of `minAvailable` / `maxUnavailable`. A budget that leaves no pod evictable (`minAvailable` ≥ `replicaCount`, `maxUnavailable: 0`) fails: it would hang every node drain. |
| `networkPolicy.enabled` | environment | `false` | With `networkPolicy.ingress` and/or `networkPolicy.egress` (rules, verbatim); each key present adds its direction to `policyTypes`. Named `serviceName`. |
| `commonLabels` | service chart | none | String labels added to every resource and the pod template, e.g. `graphql-federation/component: subgraph` for gateway discovery. The five library labels cannot be replaced. |
| `extraEnv` | service chart | none | Variables the binary reads beyond the contract, appended last (so they may reference `$(DATABASE_URL)` and the others). |
| `extraInitContainers` | service chart | none | Appended after `wait-for-postgres`; one without a `securityContext` gets the hardened container defaults. |
| `waitForPostgres.enabled`, `.image` | service chart | `true`, `busybox:1.36` | The TCP wait before boot. |
| `automountServiceAccountToken` | service chart | `false` | `true` only if the binary calls the Kubernetes API. |
| `podSecurityContext`, `containerSecurityContext` | service chart | hardened | A key set here replaces the default key of the same name, a key set to `null` removes it. |

A value that the deploying repository owns but that is identical in every
environment — typically the DSN Secret names — may be fixed in the service
chart's `values.yaml`; the library only requires that someone states it.

## Moving a pre-library chart onto the library

The nine pre-library charts of the BotResources platform services on these
crates differ from the library in three ways that change a render:

- `app.kubernetes.io/managed-by` is `Helm` (`.Release.Service`), was `helm`;
- the pod does not mount a service-account token;
- `extraEnv` entries come after the contract variables.

and in one way that changes the deploying repository's values: the trusted
Postgres host is no longer hard-coded — each environment that trusts it adds
`postgres.trustedNetwork: true` (see
[`ci/example-service/values-deploy-additions.yaml`](ci/example-service/values-deploy-additions.yaml)).

[`ci/diff-against-chart.sh`](ci/diff-against-chart.sh) renders a pre-library
chart and the example chart with the same per-environment values and prints
the normalised diff, environment by environment.

## CI

[`.github/scripts/check-chart.sh`](../../.github/scripts/check-chart.sh), job
`chart` of `ci.yml`: lint, per-environment render of
[`ci/example-service`](ci/example-service) with field-by-field assertions of
the contract, a render with every optional field, one failing render per
guard, and the version gate (a change outside `ci/` needs a version bump and a
`CHANGELOG.md` entry). `.github/workflows/chart-release.yml` publishes a new
version from `main` or `release/**`, once: a published version and its tag
`chart/br-common-service/v<version>` are never replaced.
