#!/usr/bin/env bash
# test-chart-release.sh — the release decisions of chart-release-lib.sh, case
# by case: SemVer precedence, the pull-request version gate (over throwaway git
# repositories), the tag and registry lookups (a throwaway remote, a stubbed
# helm), the package comparison (real `helm package` output) and the publish
# decision table.
#
# Runs in CI (ci.yml, job `chart`) and locally the same way. Needs bash 3.2+,
# git, yq (mikefarah v4) and helm. Touches nothing outside a temporary
# directory, and reads no global or system git configuration.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source-path=SCRIPTDIR source=chart-release-lib.sh
source "${here}/chart-release-lib.sh"

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

failures=0
passed=0

# check <label> <expected exit status> <expected stdout fragment> <command…>
check() {
  local label="$1" want_rc="$2" want="$3"
  shift 3
  local got rc=0
  got="$("$@" 2>/dev/null)" || rc=$?
  if [ "$rc" = "$want_rc" ] && [[ "$got" == *"$want"* ]]; then
    echo "  ✓ ${label}"
    passed=$((passed + 1))
  else
    echo "::error::${label}: expected exit ${want_rc} and '${want}', got exit ${rc} and '${got}'" >&2
    failures=$((failures + 1))
  fi
}

# ── SemVer precedence ───────────────────────────────────────────────────────
echo "── semver_cmp"
while read -r a b want; do
  check "${a} vs ${b}" 0 "$want" semver_cmp "$a" "$b"
done <<'EOF'
1.0.0 1.0.0 0
1.0.1 1.0.0 1
1.0.0 1.0.1 -1
1.10.0 1.9.0 1
2.0.0 1.99.99 1
0.9.0 1.0.0 -1
1.0.0 1.0.0-rc.1 1
1.0.0-rc.1 1.0.0 -1
1.0.0-rc.2 1.0.0-rc.1 1
1.0.0-rc.10 1.0.0-rc.9 1
1.0.0-alpha 1.0.0-alpha.1 -1
1.0.0-alpha.1 1.0.0-alpha.beta -1
1.0.0-beta 1.0.0-alpha.beta 1
1.0.0-rc.1 1.0.0-rc.1 0
EOF

# ── Version gate ────────────────────────────────────────────────────────────
# Each case gets its own repository: `base` is the branch a pull request
# targets, HEAD is the pull request.
echo "── chart_version_gate"

# new_repo <case> [<chart version on base>] — prints the repository path.
# Without a version, base has no chart.
new_repo() {
  local dir="${work}/gate-$1"
  git init -q -b main "$dir"
  git -C "$dir" commit -q --allow-empty -m "empty"
  if [ -n "${2:-}" ]; then
    write_chart "$dir" "$2" "base template"
    commit "$dir" "chart $2"
  fi
  git -C "$dir" branch base
  echo "$dir"
}
# write_chart <repo> <version> <template text>
write_chart() {
  mkdir -p "$1/charts/c/templates" "$1/charts/c/ci"
  printf 'apiVersion: v2\nname: c\ntype: library\nversion: %s\n' "$2" >"$1/charts/c/Chart.yaml"
  printf '%s\n' "$3" >"$1/charts/c/templates/_a.tpl"
}
commit() {
  git -C "$1" add -A
  git -C "$1" commit -q -m "$2"
}
# tag_side <repo> <tag> — the tag on a commit outside HEAD's history, as a
# release from another branch leaves it.
tag_side() {
  local head
  head="$(git -C "$1" rev-parse HEAD)"
  git -C "$1" commit -q --allow-empty -m "side"
  git -C "$1" tag "$2"
  git -C "$1" reset -q --hard "$head"
}
gate() { (cd "$1" && chart_version_gate charts/c "$2" chart/c/v); }

r="$(new_repo new)"
write_chart "$r" 1.0.0 "first"
commit "$r" "new chart"
check "new chart" 0 "is new since base (version 1.0.0)" gate "$r" base

r="$(new_repo new-stray-tag)"
git -C "$r" tag chart/c/v1.0.0
write_chart "$r" 1.0.0 "first"
commit "$r" "new chart"
check "new chart whose version is already tagged" 1 "is already released (tag chart/c/v1.0.0)" gate "$r" base

r="$(new_repo bump 1.0.0)"
write_chart "$r" 1.0.1 "changed"
commit "$r" "bump"
check "bump" 0 "version 1.0.0 -> 1.0.1" gate "$r" base

r="$(new_repo bump-prerelease 1.0.0)"
write_chart "$r" 1.1.0-rc.1 "changed"
commit "$r" "pre-release"
check "bump to a pre-release of the next minor" 0 "version 1.0.0 -> 1.1.0-rc.1" gate "$r" base

r="$(new_repo no-bump 1.0.0)"
write_chart "$r" 1.0.0 "changed"
commit "$r" "no bump"
check "no bump" 1 "version is still 1.0.0" gate "$r" base

r="$(new_repo lower 1.0.0)"
write_chart "$r" 0.9.0 "changed"
commit "$r" "lower"
check "version lower" 1 "goes from 1.0.0 to 0.9.0" gate "$r" base

r="$(new_repo prerelease-of-same 1.0.0)"
write_chart "$r" 1.0.0-rc.1 "changed"
commit "$r" "pre-release below"
check "pre-release of the released version" 1 "goes from 1.0.0 to 1.0.0-rc.1" gate "$r" base

r="$(new_repo tagged 1.0.0)"
tag_side "$r" chart/c/v1.0.1
write_chart "$r" 1.0.1 "changed"
commit "$r" "bump to a released version"
check "version already tagged" 1 "is already released (tag chart/c/v1.0.1)" gate "$r" base

r="$(new_repo not-semver 1.0.0)"
write_chart "$r" 1.1 "changed"
commit "$r" "bad version"
check "not a release version" 1 "'1.1' is not a release version" gate "$r" base

r="$(new_repo ci-only 1.0.0)"
echo "fixture" >"$r/charts/c/ci/fixture.yaml"
commit "$r" "ci only"
check "only ci/ changed" 0 "no packaged file changed since base" gate "$r" base

r="$(new_repo unchanged 1.0.0)"
git -C "$r" tag chart/c/v1.0.0
check "nothing changed (the version is released)" 0 "no packaged file changed since base (version 1.0.0)" gate "$r" base

check "no base to compare with" 2 "not found; version gate skipped" gate "$r" origin/does-not-exist

# ── Tag lookup ──────────────────────────────────────────────────────────────
echo "── chart_tag_state"
remote="${work}/remote.git"
git init -q --bare "$remote"
r="$(new_repo tag-remote)"
git -C "$r" tag chart/c/v1.0.0
git -C "$r" push -q "$remote" refs/tags/chart/c/v1.0.0
check "tag on the remote" 0 "present" chart_tag_state "$remote" chart/c/v1.0.0
check "tag not on the remote" 0 "absent" chart_tag_state "$remote" chart/c/v1.0.1
check "remote unreachable" 1 "cannot tell whether tag chart/c/v1.0.0 exists" \
  chart_tag_state "${work}/no-such-remote.git" chart/c/v1.0.0

# ── Registry lookup (helm stubbed) ──────────────────────────────────────────
echo "── chart_registry_state"
stub_bin="${work}/bin"
mkdir -p "$stub_bin"
cat >"${stub_bin}/helm" <<'EOF'
#!/usr/bin/env bash
[ -z "${STUB_HELM_STDERR:-}" ] || printf '%s\n' "$STUB_HELM_STDERR" >&2
exit "${STUB_HELM_RC:-0}"
EOF
chmod +x "${stub_bin}/helm"
# registry_says <helm exit status> <helm stderr>
registry_says() (
  export STUB_HELM_RC="$1" STUB_HELM_STDERR="$2" PATH="${stub_bin}:${PATH}"
  hash -r
  chart_registry_state oci://registry.example/charts c 1.0.0
)
fetch='Error: failed to perform "FetchReference" on source'
while IFS='|' read -r label rc stderr want_rc want; do
  check "$label" "$want_rc" "$want" registry_says "$rc" "$stderr"
done <<EOF
published|0||0|present
tag not found|1|${fetch}: registry.example/charts/c:1.0.0: not found|0|absent
manifest unknown|1|${fetch}: response status code 404: manifest unknown: manifest unknown|0|absent
repository unknown|1|${fetch}: response status code 404: name unknown: repository name not known to registry|0|absent
token denied|1|${fetch}: GET "https://registry.example/token?scope=x": response status code 403: denied: denied|1|cannot tell whether
unauthorized|1|${fetch}: response status code 401: unauthorized: authentication required|1|cannot tell whether
rate limited|1|${fetch}: response status code 429: toomanyrequests: retry later|1|cannot tell whether
server error|1|${fetch}: response status code 503: service unavailable|1|cannot tell whether
dns failure|1|${fetch}: Get "https://registry.example/v2/": dial tcp: lookup registry.example: no such host|1|cannot tell whether
timeout|1|Error: context deadline exceeded (Client.Timeout exceeded while awaiting headers)|1|cannot tell whether
not a chart|1|Error: manifest does not contain minimum number of descriptors (2), descriptors found: 0|1|cannot tell whether
EOF

# ── Package comparison (real helm package) ──────────────────────────────────
echo "── chart_packages_match"
pkg() { # pkg <out dir> <template text> — package a one-template library chart
  local src="${work}/src-$1"
  mkdir -p "$src"
  write_chart "$src" 1.0.0 "$2"
  rm -rf "$src/charts/c/ci"
  helm package "$src/charts/c" --destination "${work}/$1" >/dev/null
  echo "${work}/$1/c-1.0.0.tgz"
}
same_a="$(pkg a "template one")"
same_b="$(pkg b "template one")"
changed="$(pkg c "template two")"
# The same package, its Chart.yaml re-serialised with another key order — as
# another Helm version might write it.
reordered="${work}/reordered"
mkdir -p "$reordered"
tar -xzf "$same_a" -C "$reordered"
yq -i -P '{"version": .version, "type": .type, "name": .name, "apiVersion": .apiVersion}' "${reordered}/c/Chart.yaml"
tar -czf "${work}/reordered.tgz" -C "$reordered" c
# The same package with one file more.
extra="${work}/extra"
mkdir -p "$extra"
tar -xzf "$same_a" -C "$extra"
echo "{{- define \"c.more\" -}}{{- end -}}" >"${extra}/c/templates/_more.tpl"
tar -czf "${work}/extra.tgz" -C "$extra" c

check "same source" 0 "same" chart_packages_match "$same_a" "$same_b"
check "Chart.yaml re-serialised" 0 "same" chart_packages_match "$same_a" "${work}/reordered.tgz"
check "a template changed" 0 "different" chart_packages_match "$same_a" "$changed"
check "a file added" 0 "different" chart_packages_match "$same_a" "${work}/extra.tgz"
check "not a package" 1 "cannot extract" chart_packages_match "$same_a" "${work}/missing.tgz"

# ── Publish decision ────────────────────────────────────────────────────────
echo "── chart_publish_decision"
while IFS='|' read -r label tag registry content want_rc want; do
  check "$label" "$want_rc" "$want" chart_publish_decision "$tag" "$registry" "$content"
done <<'EOF'
not released|absent|absent||0|push
released|present|present|same|0|noop
package with no tag, same content|absent|present|same|0|tag
package with no tag, different content|absent|present|different|1|already published with different content
tag and package, different content|present|present|different|1|already published with different content
tag with no package|present|absent||1|the tag exists but the registry does not hold this version
registry present without a comparison|absent|present||1|invalid inputs
unknown state|error|absent||1|invalid inputs
EOF

echo
if [ "$failures" -gt 0 ]; then
  echo "✗ ${failures} of $((passed + failures)) case(s) failed" >&2
  exit 1
fi
echo "✓ ${passed} release-decision cases passed"
