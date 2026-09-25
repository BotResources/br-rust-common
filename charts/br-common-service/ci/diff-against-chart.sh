#!/usr/bin/env bash
# diff-against-chart.sh — compare the example service chart (br-common-service)
# with a pre-library service chart, environment by environment.
#
# Both charts are rendered with the SAME per-environment values file — the one
# the deploying repository keeps for the pre-library chart — so every line of
# the diff comes from the templates, not from the values. The example chart
# also gets its own values.yaml (what the binary defines) and
# example-service/values-deploy-additions.yaml (what a deploying repository
# must add when it moves onto the library).
#
# Both renders are normalised before the diff: YAML comments dropped, map keys
# sorted, documents ordered by kind then name. List order is KEPT — the order
# of `env` entries matters to Kubernetes for $(VAR) expansion.
#
# Usage:
#   diff-against-chart.sh --chart <pre-library chart dir> \
#                         --values-dir <dir holding <env>/values.yaml> \
#                         [--envs "dev uat prod"] [--kube-version 1.34.0] \
#                         [--fail-on-diff]
#
# Example, from a checkout of the deploying repository:
#   diff-against-chart.sh --chart <deploy-repo>/<path>/charter/chart \
#                         --values-dir <deploy-repo>/<path>/charter
#
# Exit status: 0 when every render succeeded (diffs are printed), 1 when a
# render failed, or when --fail-on-diff is given and a diff is not empty.
# Needs helm and yq (mikefarah, v4).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
example="${here}/example-service"

legacy_chart=""
values_dir=""
envs="dev uat prod"
kube_version="1.34.0"
fail_on_diff=false

while [ $# -gt 0 ]; do
  case "$1" in
    --chart) legacy_chart="$2"; shift 2 ;;
    --values-dir) values_dir="$2"; shift 2 ;;
    --envs) envs="$2"; shift 2 ;;
    --kube-version) kube_version="$2"; shift 2 ;;
    --fail-on-diff) fail_on_diff=true; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [ -z "$legacy_chart" ] || [ -z "$values_dir" ]; then
  echo "usage: $0 --chart <pre-library chart dir> --values-dir <dir holding <env>/values.yaml>" >&2
  exit 2
fi

for tool in helm yq; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 2; }
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

helm dependency build "$example" >/dev/null

normalise() {
  yq eval-all -P '[.] | map(select(. != null)) | sort_by(.kind, .metadata.name) | .[] | ... comments="" | sort_keys(..) | split_doc' -
}

status=0
for env in $envs; do
  env_values="${values_dir}/${env}/values.yaml"
  if [ ! -f "$env_values" ]; then
    echo "::error::${env_values} does not exist" >&2
    exit 1
  fi
  # The same release name on both sides: app.kubernetes.io/instance is part of
  # the immutable Deployment selector, so it must not differ.
  release="release-${env}"

  helm template "$release" "$legacy_chart" \
    --kube-version "$kube_version" \
    -f "$env_values" | normalise > "${work}/${env}-pre-library.yaml"

  helm template "$release" "$example" \
    --kube-version "$kube_version" \
    -f "$env_values" \
    -f "${example}/values-deploy-additions.yaml" | normalise > "${work}/${env}-library.yaml"

  echo "===== ${env}: pre-library chart (-) vs br-common-service example (+)"
  if diff -u \
      --label "pre-library/${env}" "${work}/${env}-pre-library.yaml" \
      --label "br-common-service/${env}" "${work}/${env}-library.yaml"; then
    echo "(identical)"
  elif [ "$fail_on_diff" = true ]; then
    status=1
  fi
done

exit "$status"
