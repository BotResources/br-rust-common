#!/usr/bin/env bash
# check-chart.sh — the gate of the br-common-service library chart.
#
#   1. helm lint the library, and check the package leaves ci/ out;
#   2. build the example service chart (file:// dependency) and lint it with
#      the values of each environment;
#   3. render it per environment and assert the ops contract, field by field;
#   4. render it with every optional field set;
#   5. prove each render guard: a missing per-environment value, an unsafe
#      combination or a forbidden override fails the render with a message
#      that names it;
#   6. version gate (chart_version_gate in chart-release-lib.sh): a change
#      under charts/br-common-service/ (ci/ excepted) needs a Chart.yaml
#      version greater than the base's and not yet tagged; every version needs
#      a `## [<version>]` heading in the chart's CHANGELOG.md.
#
# Runs in CI (ci.yml, job `chart`) and in the release workflow before a push;
# runs locally the same way. Needs helm (v3 or v4), yq (mikefarah, v4), git.
# The release decisions themselves are tested by test-chart-release.sh.
#
# shellcheck disable=SC2016 # '$(PGUSER)' below is Kubernetes syntax, compared literally
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
# shellcheck source-path=SCRIPTDIR source=chart-release-lib.sh
source "${repo_root}/.github/scripts/chart-release-lib.sh"

chart_dir="charts/br-common-service"
example="${chart_dir}/ci/example-service"
envs="dev uat prod"

failures=0
fail() {
  echo "::error::$*" >&2
  failures=$((failures + 1))
}
ok() { echo "  ✓ $*"; }

# ── 1. The library ──────────────────────────────────────────────────────────
echo "── helm lint ${chart_dir}"
helm lint --strict "$chart_dir"

echo "── the package leaves the CI fixture out"
pkg_dir="$(mktemp -d)"
trap 'rm -rf "$pkg_dir"' EXIT
helm package "$chart_dir" --destination "$pkg_dir" >/dev/null
if tar -tzf "$pkg_dir"/br-common-service-*.tgz | grep -q '/ci/'; then
  fail "the packaged library contains ci/ — check ${chart_dir}/.helmignore"
else
  ok "no ci/ in the package"
fi

# ── 2. The example service chart ────────────────────────────────────────────
echo "── helm dependency build ${example}"
# charts/ and Chart.lock are build output (gitignored): rebuild them from the
# library in this checkout, never from a stale local build.
rm -rf "${example}/charts" "${example}/Chart.lock"
helm dependency build --skip-refresh "$example" >/dev/null

for env in $envs; do
  echo "── helm lint ${example} (${env})"
  helm lint --strict "$example" -f "${example}/values-${env}.yaml"
done

render() {
  helm template example "$example" "$@"
}

# copy_example <dir> — a copy of the example service chart in <dir>, its
# library dependency rebuilt from this checkout, for a Chart.yaml the caller
# edits (an annotation cannot be set from the command line).
copy_example() {
  cp -R "$example" "${1}/example-service"
  rm -rf "${1}/example-service/charts" "${1}/example-service/Chart.lock"
  sed -i.bak 's#file://../..#file://'"${repo_root}/${chart_dir}"'#' "${1}/example-service/Chart.yaml"
  helm dependency build --skip-refresh "${1}/example-service" >/dev/null
}

# yq over the one document of a kind: q <render> <kind> <expression>.
# Prints nothing when the render has no document of that kind.
q() {
  local doc
  doc="$(yq eval "select(.kind == \"$2\")" - <<<"$1")"
  [ -n "$doc" ] || return 0
  yq eval "$3" - <<<"$doc"
}

expect() {
  local what="$1" want="$2" got="$3"
  if [ "$got" = "$want" ]; then
    ok "${what} = ${want}"
  else
    fail "${what}: expected '${want}', got '${got}'"
  fi
}

# ── 3. The ops contract, per environment ────────────────────────────────────
# The expected DSNs. $(PGUSER) and the others are Kubernetes variable
# references the kubelet expands at container start — no credential here, hence
# the secret scanner's ignore marker on each line.
app_dsn='postgres://$(PGUSER):$(PGPASSWORD)@pg-rw:5432/charter'                   # trufflehog:ignore
owner_dsn='postgres://$(PGUSER_OWNER):$(PGPASSWORD_OWNER)@pg-rw:5432/charter'     # trufflehog:ignore
app_dsn_port='postgres://$(PGUSER):$(PGPASSWORD)@pg-rw:6432/charter'              # trufflehog:ignore
app_dsn_tls='postgres://$(PGUSER):$(PGPASSWORD)@pg-rw:5432/charter?sslmode=require'                    # trufflehog:ignore
app_dsn_verify='postgres://$(PGUSER):$(PGPASSWORD)@pg-rw:5432/charter?sslmode=verify-full'             # trufflehog:ignore
owner_dsn_verify='postgres://$(PGUSER_OWNER):$(PGPASSWORD_OWNER)@pg-rw:5432/charter?sslmode=verify-full' # trufflehog:ignore
# busybox:1.36, pinned to the digest of its multi-arch index (amd64, arm64, …).
wait_image='busybox:1.36@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662'
for env in $envs; do
  echo "── ops contract (${env})"
  out="$(render -f "${example}/values-${env}.yaml")"
  c='.spec.template.spec.containers[0]'
  expect "kinds" "Deployment Service ServiceAccount" \
    "$(yq eval '.kind' - <<<"$out" | grep -v '^---$' | sort | tr '\n' ' ' | sed 's/ $//')"
  expect "selector name" "charter" "$(q "$out" Deployment '.spec.selector.matchLabels["app.kubernetes.io/name"]')"
  expect "selector instance" "example" "$(q "$out" Deployment '.spec.selector.matchLabels["app.kubernetes.io/instance"]')"
  expect "selector size" "2" "$(q "$out" Deployment '.spec.selector.matchLabels | length')"
  expect "env label" "$env" "$(q "$out" Deployment '.spec.template.metadata.labels["botresources.ai/env"]')"
  expect "common label on the Service" "subgraph" "$(q "$out" Service '.metadata.labels["graphql-federation/component"]')"
  expect "Service port name" "http" "$(q "$out" Service '.spec.ports[0].name')"
  expect "Service port" "8004" "$(q "$out" Service '.spec.ports[0].port')"
  expect "container port" "8004" "$(q "$out" Deployment "${c}.ports[0].containerPort")"
  expect "PORT" "8004" "$(q "$out" Deployment "${c}.env[] | select(.name == \"PORT\") | .value")"
  expect "ENVIRONMENT" "$env" "$(q "$out" Deployment "${c}.env[] | select(.name == \"ENVIRONMENT\") | .value")"
  expect "image" "ghcr.io/botresources/br-svc-charter:0.5.3" "$(q "$out" Deployment "${c}.image")"
  expect "readiness path" "/readyz" "$(q "$out" Deployment "${c}.readinessProbe.httpGet.path")"
  expect "liveness path (service override)" "/health" "$(q "$out" Deployment "${c}.livenessProbe.httpGet.path")"
  expect "no startup probe by default" "null" "$(q "$out" Deployment "${c}.startupProbe")"
  # Exact values: on a trusted network, neither DSN carries an sslmode.
  expect "DATABASE_URL" "$app_dsn" \
    "$(q "$out" Deployment "${c}.env[] | select(.name == \"DATABASE_URL\") | .value")"
  expect "DATABASE_URL_OWNER" "$owner_dsn" \
    "$(q "$out" Deployment "${c}.env[] | select(.name == \"DATABASE_URL_OWNER\") | .value")"
  expect "app DSN reads the app Secret" "pg-charter-app-credentials" \
    "$(q "$out" Deployment "${c}.env[] | select(.name == \"PGPASSWORD\") | .valueFrom.secretKeyRef.name")"
  expect "owner DSN reads the owner Secret" "pg-charter-owner-credentials" \
    "$(q "$out" Deployment "${c}.env[] | select(.name == \"PGPASSWORD_OWNER\") | .valueFrom.secretKeyRef.name")"
  expect "credentials precede the DSNs" "PGUSER PGPASSWORD DATABASE_URL PGUSER_OWNER PGPASSWORD_OWNER DATABASE_URL_OWNER" \
    "$(q "$out" Deployment "[${c}.env[].name | select(test(\"^(PG|DATABASE)\"))] | join(\" \")")"
  expect "TRUSTED_NETWORK_HOSTS" "pg-rw" "$(q "$out" Deployment "${c}.env[] | select(.name == \"TRUSTED_NETWORK_HOSTS\") | .value")"
  expect "NATS_URL" "nats://nats:4222" "$(q "$out" Deployment "${c}.env[] | select(.name == \"NATS_URL\") | .value")"
  expect "extra env last" "SCOPE_DECLARATION_ENABLED" "$(q "$out" Deployment "${c}.env[-1].name")"
  expect "reload annotation" "pg-charter-owner-credentials,pg-charter-app-credentials,registry-pull" \
    "$(q "$out" Deployment '.metadata.annotations["secret.reloader.stakater.com/reload"]')"
  expect "wait-for-postgres" "wait-for-postgres" "$(q "$out" Deployment '.spec.template.spec.initContainers[0].name')"
  expect "wait image pinned by digest" "$wait_image" "$(q "$out" Deployment '.spec.template.spec.initContainers[0].image')"
  expect "no token mounted" "false" "$(q "$out" Deployment '.spec.template.spec.automountServiceAccountToken')"
  expect "runAsNonRoot" "true" "$(q "$out" Deployment '.spec.template.spec.securityContext.runAsNonRoot')"
  expect "readOnlyRootFilesystem" "true" "$(q "$out" Deployment "${c}.securityContext.readOnlyRootFilesystem")"
  expect "no PDB by default" "" "$(q "$out" PodDisruptionBudget '.metadata.name')"
  expect "no NetworkPolicy by default" "" "$(q "$out" NetworkPolicy '.metadata.name')"
  expect "no strategy by default" "null" "$(q "$out" Deployment '.spec.strategy')"
done

# ── 4. Every optional field ─────────────────────────────────────────────────
echo "── every optional field"
out="$(render -f "${example}/values-prod.yaml" -f "${example}/values-all-fields.yaml")"
c='.spec.template.spec.containers[0]'
expect "args" "serve" "$(q "$out" Deployment "${c}.args[0]")"
expect "strategy" "Recreate" "$(q "$out" Deployment '.spec.strategy.type')"
expect "replicas" "3" "$(q "$out" Deployment '.spec.replicas')"
expect "pullPolicy" "Always" "$(q "$out" Deployment "${c}.imagePullPolicy")"
expect "readiness path override" "/ready" "$(q "$out" Deployment "${c}.readinessProbe.httpGet.path")"
expect "readiness period" "7" "$(q "$out" Deployment "${c}.readinessProbe.periodSeconds")"
expect "readiness delay kept" "5" "$(q "$out" Deployment "${c}.readinessProbe.initialDelaySeconds")"
expect "liveness timeout" "3" "$(q "$out" Deployment "${c}.livenessProbe.timeoutSeconds")"
expect "startup path" "/livez" "$(q "$out" Deployment "${c}.startupProbe.httpGet.path")"
expect "startup failureThreshold" "60" "$(q "$out" Deployment "${c}.startupProbe.failureThreshold")"
expect "startup period kept" "5" "$(q "$out" Deployment "${c}.startupProbe.periodSeconds")"
expect "postgres port" "$app_dsn_port" \
  "$(q "$out" Deployment "${c}.env[] | select(.name == \"DATABASE_URL\") | .value")"
expect "app password env" "pg-charter-app-credentials" \
  "$(q "$out" Deployment "${c}.env[] | select(.name == \"EXAMPLE_APP_PASSWORD\") | .valueFrom.secretKeyRef.name")"
expect "app password before DATABASE_URL" "PGUSER PGPASSWORD EXAMPLE_APP_PASSWORD DATABASE_URL" \
  "$(q "$out" Deployment "[${c}.env[].name | select(test(\"^(PG|DATABASE|EXAMPLE)\") and (test(\"OWNER\") | not))] | join(\" \")")"
expect "wait image" "registry.example.com/tools/busybox:1.37" "$(q "$out" Deployment '.spec.template.spec.initContainers[0].image')"
expect "extra init container" "check-owner" "$(q "$out" Deployment '.spec.template.spec.initContainers[1].name')"
expect "extra init container hardened" "ALL" "$(q "$out" Deployment '.spec.template.spec.initContainers[1].securityContext.capabilities.drop[0]')"
expect "reload annotation (every Secret the pod reads)" \
  "pg-charter-owner-credentials,pg-charter-app-credentials,blob-credentials,owner-check-credentials,registry-pull" \
  "$(q "$out" Deployment '.metadata.annotations["secret.reloader.stakater.com/reload"]')"
expect "token mounted on request" "true" "$(q "$out" Deployment '.spec.template.spec.automountServiceAccountToken')"
expect "runAsUser override" "10001" "$(q "$out" Deployment '.spec.template.spec.securityContext.runAsUser')"
expect "runAsNonRoot kept" "true" "$(q "$out" Deployment '.spec.template.spec.securityContext.runAsNonRoot')"
expect "readOnlyRootFilesystem false honoured" "false" "$(q "$out" Deployment "${c}.securityContext.readOnlyRootFilesystem")"
expect "topology spread" "kubernetes.io/hostname" "$(q "$out" Deployment '.spec.template.spec.topologySpreadConstraints[0].topologyKey')"
expect "nodeSelector" "arm64" "$(q "$out" Deployment '.spec.template.spec.nodeSelector["kubernetes.io/arch"]')"
expect "tolerations" "example.com/dedicated" "$(q "$out" Deployment '.spec.template.spec.tolerations[0].key')"
expect "PDB" "2" "$(q "$out" PodDisruptionBudget '.spec.minAvailable')"
expect "PDB selector" "charter" "$(q "$out" PodDisruptionBudget '.spec.selector.matchLabels["app.kubernetes.io/name"]')"
expect "NetworkPolicy types" "Egress" "$(q "$out" NetworkPolicy '.spec.policyTypes | join(",")')"
expect "NetworkPolicy egress port" "9000" "$(q "$out" NetworkPolicy '.spec.egress[0].ports[0].port')"
expect "NetworkPolicy has no ingress key" "false" "$(q "$out" NetworkPolicy '.spec | has("ingress")')"

echo "── a service that never migrates, on a TLS Postgres"
out="$(render -f "${example}/values-dev.yaml" --set postgres.migrate=false --set postgres.ownerSecretName=null \
  --set postgres.trustedNetwork=false --set postgres.sslMode=require)"
expect "DATABASE_URL carries the TLS mode" "$app_dsn_tls" \
  "$(q "$out" Deployment "${c}.env[] | select(.name == \"DATABASE_URL\") | .value")"
expect "no DATABASE_URL_OWNER" "0" "$(q "$out" Deployment "[${c}.env[] | select(.name | test(\"OWNER\"))] | length")"
expect "no TRUSTED_NETWORK_HOSTS" "0" "$(q "$out" Deployment "[${c}.env[] | select(.name == \"TRUSTED_NETWORK_HOSTS\")] | length")"
expect "reload annotation without owner" "pg-charter-app-credentials,registry-pull" \
  "$(q "$out" Deployment '.metadata.annotations["secret.reloader.stakater.com/reload"]')"

echo "── a migrating service on a TLS Postgres that verifies the server"
out="$(render -f "${example}/values-dev.yaml" --set postgres.trustedNetwork=false --set postgres.sslMode=verify-full)"
expect "DATABASE_URL carries the TLS mode" "$app_dsn_verify" \
  "$(q "$out" Deployment "${c}.env[] | select(.name == \"DATABASE_URL\") | .value")"
expect "DATABASE_URL_OWNER carries the TLS mode" "$owner_dsn_verify" \
  "$(q "$out" Deployment "${c}.env[] | select(.name == \"DATABASE_URL_OWNER\") | .value")"

echo "── image references: a digest pin, a local build on explicit request"
# Any digest: the chart checks its shape, never its value.
digest="sha256:$(printf '%064d' 0 | tr 0 a)"
repo='ghcr.io/botresources/br-svc-charter'
out="$(render -f "${example}/values-dev.yaml" --set-string "image.tag=0.5.3@${digest}")"
expect "a version pinned by digest, the check enforced" "${repo}:0.5.3@${digest}" "$(q "$out" Deployment "${c}.image")"
out="$(render -f "${example}/values-dev.yaml" --set-string image.tag=v0.5.4)"
expect "a leading v" "${repo}:v0.5.4" "$(q "$out" Deployment "${c}.image")"
out="$(render -f "${example}/values-dev.yaml" --set image.tag=local-build --set image.enforceSupportedVersions=false)"
expect "local tag" "${repo}:local-build" "$(q "$out" Deployment "${c}.image")"
for tag in local.4f2a9c1 dev-4f2a9c1; do
  out="$(render -f "${example}/values-dev.yaml" --set-string "image.tag=${tag}" --set image.enforceSupportedVersions=false)"
  expect "local tag ${tag}" "${repo}:${tag}" "$(q "$out" Deployment "${c}.image")"
done
out="$(render -f "${example}/values-dev.yaml" --set-string "image.tag=dev.4f2a9c1@${digest}" --set image.enforceSupportedVersions=false)"
expect "local tag pinned by digest" "${repo}:dev.4f2a9c1@${digest}" "$(q "$out" Deployment "${c}.image")"
out="$(render -f "${example}/values-dev.yaml" --set-string image.tag=0.5.3 --set image.enforceSupportedVersions=false)"
expect "a version in range, the check lifted" "${repo}:0.5.3" "$(q "$out" Deployment "${c}.image")"
# A pre-release is inside a range that names a pre-release (Masterminds/Kargo);
# the guards below prove it is outside the fixture's `>=0.5.0 <0.6.0`.
pre="$(mktemp -d)"
copy_example "$pre"
yq -i '.annotations["botresources.ai/supported-app-versions"] = ">=0.5.0-0 <0.6.0"' "${pre}/example-service/Chart.yaml"
out="$(helm template example "${pre}/example-service" -f "${example}/values-dev.yaml" --set-string image.tag=0.5.4-rc.1)"
expect "a pre-release, inside a pre-release range" "${repo}:0.5.4-rc.1" "$(q "$out" Deployment "${c}.image")"
rm -rf "$pre"

echo "── PodDisruptionBudget bounds: percentages, quoted integers"
all=(-f "${example}/values-prod.yaml" -f "${example}/values-all-fields.yaml")
out="$(render "${all[@]}" --set-string podDisruptionBudget.minAvailable=66%)"
expect "percentage kept as a string" '"66%"' "$(q "$out" PodDisruptionBudget '.spec.minAvailable | to_json')"
out="$(render "${all[@]}" --set-string podDisruptionBudget.minAvailable=2)"
expect "quoted integer rendered as an integer" "2" "$(q "$out" PodDisruptionBudget '.spec.minAvailable | to_json')"
out="$(render "${all[@]}" --set podDisruptionBudget.minAvailable=null --set-string podDisruptionBudget.maxUnavailable=1%)"
expect "maxUnavailable percentage" '"1%"' "$(q "$out" PodDisruptionBudget '.spec.maxUnavailable | to_json')"
# No pod, nothing to evict: an environment scaled to 0 keeps its budget.
out="$(render -f "${example}/values-dev.yaml" --set replicaCount=0 --set podDisruptionBudget.enabled=true --set podDisruptionBudget.minAvailable=1)"
expect "minAvailable kept at replicaCount 0" "1" "$(q "$out" PodDisruptionBudget '.spec.minAvailable')"
out="$(render -f "${example}/values-dev.yaml" --set replicaCount=0 --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.minAvailable=100%)"
expect "minAvailable 100% kept at replicaCount 0" '"100%"' "$(q "$out" PodDisruptionBudget '.spec.minAvailable | to_json')"

# ── 5. Render guards ────────────────────────────────────────────────────────
# must_fail <message fragment> <helm template args…>
must_fail() {
  local want="$1"
  shift
  local err
  if err="$(render "$@" 2>&1 >/dev/null)"; then
    fail "render succeeded, expected a failure naming '${want}' (args: $*)"
  elif grep -qF -- "$want" <<<"$err"; then
    ok "refused: ${want}"
  else
    fail "render failed without naming '${want}': ${err}"
  fi
}

dev=(-f "${example}/values-dev.yaml")
echo "── required values"
for key in env image.tag replicaCount nats.url postgres.host postgres.port postgres.trustedNetwork \
  serviceName port image.repository postgres.database postgres.appSecretName postgres.ownerSecretName; do
  must_fail "${key} is required" "${dev[@]}" --set "${key}=null"
done
must_fail "postgres.sslMode is required when postgres.trustedNetwork is false" "${dev[@]}" --set postgres.trustedNetwork=false
must_fail "resources is required" "${dev[@]}" --set resources=null

echo "── guards"
must_fail "is outside the range" "${dev[@]}" --set image.tag=0.6.0
must_fail "is outside the range" "${dev[@]}" --set-string "image.tag=0.6.0@${digest}"
# A pre-release is outside a range that names none, even between its bounds.
must_fail "is outside the range" "${dev[@]}" --set-string image.tag=0.5.4-rc.1
must_fail "is not a release version" "${dev[@]}" --set image.tag=latest
must_fail "is not a release version" "${dev[@]}" --set-string image.tag=0.5
# SemVer build metadata renders, then fails at the kubelet: an OCI tag has no '+'.
must_fail "is not an image tag" "${dev[@]}" --set-string image.tag=0.5.3+build.1
# A digest in upper-case hex, a short one, one without a tag.
must_fail "is not an image tag" "${dev[@]}" --set-string "image.tag=0.5.3@sha256:$(printf '%064d' 0 | tr 0 A)"
must_fail "is not an image tag" "${dev[@]}" --set-string image.tag=0.5.3@sha256:abc
must_fail "is not an image tag" "${dev[@]}" --set-string "image.tag=@${digest}"
# enforceSupportedVersions=false admits a local build (local-…, dev-…) and
# nothing else: never a version outside the range, digest or not, never a tag
# that starts like a version without being one, never `latest`.
lifted=("${dev[@]}" --set image.enforceSupportedVersions=false)
must_fail "is outside the range" "${lifted[@]}" --set image.tag=0.9.0
must_fail "is outside the range" "${lifted[@]}" --set-string "image.tag=0.9.0@${digest}"
must_fail "is not a release version: a version tag is" "${lifted[@]}" --set-string image.tag=0.9
must_fail "is not a release version: a version tag is" "${lifted[@]}" --set-string image.tag=0.9.0_x
must_fail "is not a release version: a version tag is" "${lifted[@]}" --set-string image.tag=v0.5.3-01
must_fail "neither a release version nor a local build tag" "${lifted[@]}" --set image.tag=latest
must_fail "neither a release version nor a local build tag" "${lifted[@]}" --set image.tag=localbuild
must_fail "is not an image tag" "${lifted[@]}" --set-string image.tag=0.5.3+build.1
must_fail "image.enforceSupportedVersions must be a YAML boolean" "${dev[@]}" --set-string image.enforceSupportedVersions=false
must_fail "exceeds maxReplicas" "${dev[@]}" --set maxReplicas=1 --set replicaCount=2
must_fail "leaves no pod evictable" "${dev[@]}" --set podDisruptionBudget.enabled=true --set podDisruptionBudget.minAvailable=1
must_fail "leaves no pod evictable" "${dev[@]}" --set podDisruptionBudget.enabled=true --set podDisruptionBudget.maxUnavailable=0
# The same budgets as a percentage or a quoted integer (replicaCount 1 in dev; 67% of 3 rounds up to 3).
must_fail "minAvailable 100% with replicaCount 1" "${dev[@]}" --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.minAvailable=100%
must_fail "minAvailable 1 with replicaCount 1" "${dev[@]}" --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.minAvailable=1
must_fail "minAvailable 67% with replicaCount 3" "${all[@]}" --set-string podDisruptionBudget.minAvailable=67%
must_fail "maxUnavailable 0% leaves no pod evictable" "${dev[@]}" --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.maxUnavailable=0%
must_fail "maxUnavailable 0 leaves no pod evictable" "${dev[@]}" --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.maxUnavailable=0
must_fail "maxUnavailable 0 leaves no pod evictable, at any replicaCount" "${dev[@]}" --set replicaCount=0 \
  --set podDisruptionBudget.enabled=true --set podDisruptionBudget.maxUnavailable=0
must_fail "must be an integer or a percentage" "${dev[@]}" --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.minAvailable=half
must_fail "must be an integer or a percentage" "${dev[@]}" --set podDisruptionBudget.enabled=true --set podDisruptionBudget.maxUnavailable=-1
must_fail "is above 100%" "${dev[@]}" --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.maxUnavailable=150%
must_fail "must be an integer or a percentage" "${dev[@]}" --set podDisruptionBudget.enabled=true --set-string podDisruptionBudget.maxUnavailable=010%
must_fail "set exactly one of minAvailable and maxUnavailable" "${dev[@]}" --set podDisruptionBudget.enabled=true
must_fail "must not set app.kubernetes.io/name" "${dev[@]}" --set 'commonLabels.app\.kubernetes\.io/name=x'
must_fail "must be a string" "${dev[@]}" --set 'commonLabels.example\.com/flag=true'
must_fail "is not a probe timing field" "${dev[@]}" --set probes.liveness.httpGet.path=/x
must_fail "must be an absolute HTTP path" "${dev[@]}" --set probes.readinessPath=readyz
must_fail "postgres.trustedNetwork must be a YAML boolean" "${dev[@]}" --set-string postgres.trustedNetwork=true
must_fail "postgres.sslMode must be require, verify-ca or verify-full" "${dev[@]}" --set postgres.trustedNetwork=false --set postgres.sslMode=prefer
must_fail "postgres.sslMode is set but postgres.trustedNetwork is true" "${dev[@]}" --set postgres.sslMode=require
must_fail "name the same Secret" "${dev[@]}" --set postgres.ownerSecretName=pg-charter-app-credentials
must_fail "neither networkPolicy.ingress nor networkPolicy.egress" "${dev[@]}" --set networkPolicy.enabled=true
must_fail "is not a DNS-1035 label" "${dev[@]}" --set serviceName=Charter_Svc
must_fail "port must be a TCP port" "${dev[@]}" --set port=70000
must_fail "is not an environment variable name" "${dev[@]}" --set postgres.appPasswordEnv=app-password

echo "── extraEnv cannot replace a contract variable"
# Kubernetes keeps the LAST of two env entries with the same name: each of
# these would silently replace what the library renders.
for name in ENVIRONMENT PORT TRUSTED_NETWORK_HOSTS PGUSER PGPASSWORD DATABASE_URL \
  PGUSER_OWNER PGPASSWORD_OWNER DATABASE_URL_OWNER NATS_URL; do
  must_fail "extraEnv must not set ${name}" "${dev[@]}" \
    --set "extraEnv[0].name=${name}" --set "extraEnv[0].value=x"
done
must_fail "extraEnv must not set ALLOW_INSECURE_DATABASE" "${dev[@]}" \
  --set extraEnv[0].name=ALLOW_INSECURE_DATABASE --set-string extraEnv[0].value=true
must_fail "extraEnv must not set EXAMPLE_APP_PASSWORD: postgres.appPasswordEnv" "${dev[@]}" \
  --set postgres.appPasswordEnv=EXAMPLE_APP_PASSWORD --set extraEnv[0].name=EXAMPLE_APP_PASSWORD --set extraEnv[0].value=x
must_fail "extraEnv declares SCOPE_DECLARATION_ENABLED twice" "${dev[@]}" \
  --set extraEnv[0].name=SCOPE_DECLARATION_ENABLED --set-string extraEnv[0].value=true \
  --set extraEnv[1].name=SCOPE_DECLARATION_ENABLED --set-string extraEnv[1].value=false
must_fail "extraEnv[0] has no name" "${dev[@]}" --set extraEnv[0].value=x
must_fail "postgres.appPasswordEnv must not be DATABASE_URL" "${dev[@]}" --set postgres.appPasswordEnv=DATABASE_URL

echo "── a chart without the supported-range annotation"
bare="$(mktemp -d)"
copy_example "$bare"
yq -i 'del(.annotations)' "${bare}/example-service/Chart.yaml"
if err="$(helm template example "${bare}/example-service" "${dev[@]}" 2>&1 >/dev/null)"; then
  fail "a chart without botresources.ai/supported-app-versions rendered"
elif grep -qF "botresources.ai/supported-app-versions is required" <<<"$err"; then
  ok "refused: the supported-range annotation is required"
else
  fail "missing annotation failed without naming it: ${err}"
fi
rm -rf "$bare"

# ── 6. Version gate ─────────────────────────────────────────────────────────
echo "── version gate"
version="$(yq '.version' "${chart_dir}/Chart.yaml")"
if grep -qF -- "## [${version}]" "${chart_dir}/CHANGELOG.md"; then
  ok "CHANGELOG.md has ## [${version}]"
else
  fail "${chart_dir}/CHANGELOG.md has no '## [${version}]' heading"
fi

gate_rc=0
gate_msg="$(chart_version_gate "$chart_dir" "origin/${GITHUB_BASE_REF:-main}" "chart/br-common-service/v")" || gate_rc=$?
case "$gate_rc" in
  0) ok "$gate_msg" ;;
  2) echo "::notice::${gate_msg}" ;;
  *) fail "$gate_msg" ;;
esac

if [ "$failures" -gt 0 ]; then
  echo "✗ ${failures} check(s) failed" >&2
  exit 1
fi
echo "✓ br-common-service chart gate passed"
