#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/test-lib.sh"

make_fixture
install_stubs

detect() {
    in_dev env GITHUB_REF="$1" EVENT_NAME="${2:-push}" GITHUB_SHA="$(g rev-parse HEAD)" \
        INPUT_CRATE="${3:-}" INPUT_VERSION="${4:-}" bash .github/scripts/release-detect.sh
}

matrix_of() {
    sed -n 's/^matrix=//p' <<<"$OUT" | head -1
}

echo "== pre-scaffold: nothing tracked, every crate at 0.0.0"
write_crate br-private 'publish = false\n'
set_version br-private 3.0.0
set_tracked ""
commit_all "empty list" >/dev/null
expect 0 'matrix=[]' "main, empty TRACKED_LIBRARIES, all 0.0.0: clean no-op" detect refs/heads/main
expect 0 'matrix=[]' "an untracked publish = false crate at 3.0.0 is exempt from tracking" detect refs/heads/main
set_tracked "br-alpha br-beta br-delta"
commit_all "track all" >/dev/null
expect 0 'matrix=[]' "main, all tracked at 0.0.0: clean no-op" detect refs/heads/main

echo "== an untracked crate that declares a release fails detection"
set_tracked "br-beta br-delta"
set_version br-alpha 0.1.0
commit_all "alpha 0.1.0, not tracked" >/dev/null
expect 1 'is not in TRACKED_LIBRARIES' "main, br-alpha 0.1.0 untracked" detect refs/heads/main

echo "== a tracked crate with an untagged version is released"
set_tracked "br-alpha br-beta br-delta"
commit_all "track alpha" >/dev/null
run detect refs/heads/main
if [[ "$RC" -eq 0 ]] && python3 -c '
import json, sys
m = json.loads(sys.argv[1])
assert [(e["crate"], e["version"], e["line"]) for e in m] == [("br-alpha", "0.1.0", "main")], m
assert len(m[0]["commit"]) == 40
' "$(matrix_of)"; then verdict yes "main: exactly br-alpha 0.1.0 on main at HEAD"; else verdict no "main: exactly br-alpha 0.1.0 on main at HEAD"; fi

echo "== already tagged and unchanged: nothing to do"
g tag -a br-alpha/v0.1.0 -m t
commit_all "unrelated" >/dev/null
expect 0 'matrix=[]' "tag exists, version unchanged" detect refs/heads/main
g push -q origin br-alpha/v0.1.0

echo "== tags of other shapes are neither a release nor a baseline"
g tag -a chart/br-rust-common-chart/v2.0.0 -m chart
g tag -a chart/br-alpha/v9.9.9 -m chart
g tag -a chart/br-beta/v9.9.9 -m chart
g tag -a v1.4.0 -m workspace
g tag -a br-beta-v0.7.0 -m legacy
g tag -a br-beta/v0.2.0-rc.1 -m prerelease
expect 0 'matrix=[]' "chart, workspace and legacy tags change nothing for the detection" detect refs/heads/main
expect_equal "$(in_dev bash .github/scripts/previous-tag.sh br-alpha)" "br-alpha/v0.1.0" "previous tag of br-alpha ignores chart/br-alpha/v9.9.9 and v1.4.0"
expect_equal "$(in_dev bash .github/scripts/previous-tag.sh br-beta)" "" "br-beta has no baseline: chart, legacy and prerelease tags are not baselines"
expect 0 'first release' "semver gate on br-beta: first release, no baseline" in_dev bash .github/scripts/semver-check.sh br-beta

echo "== the release-line law"
g branch release/br-alpha-0.1 "br-alpha/v0.1.0"
g push -q origin release/br-alpha-0.1
set_version br-alpha 0.1.1
commit_all "alpha 0.1.1 on main" >/dev/null
run detect refs/heads/main
if [[ "$RC" -eq 0 && "$(matrix_of)" == '[]' ]] && grep -q '^refused=\["br-alpha 0.1.1' <<<"$OUT"; then verdict yes "main may not publish br-alpha 0.1.1: refused, scan goes on"; else verdict no "main may not publish br-alpha 0.1.1: refused, scan goes on"; fi
set_version br-alpha 0.2.0
commit_all "alpha 0.2.0 on main" >/dev/null
run detect refs/heads/main
if grep -q '"version":"0.2.0","line":"main"' <<<"$OUT"; then verdict yes "main publishes br-alpha 0.2.0"; else verdict no "main publishes br-alpha 0.2.0"; fi

echo "== a release line publishes its own crate only"
g checkout -q release/br-alpha-0.1
set_tracked "br-alpha br-beta br-delta"
set_version br-alpha 0.1.1
set_version br-beta 0.3.0
commit_all "hotfix and a stray beta bump" >/dev/null
run detect refs/heads/release/br-alpha-0.1
if grep -q '"crate":"br-alpha","version":"0.1.1","line":"release/br-alpha-0.1"' <<<"$OUT" && ! grep -q 'br-beta","version' <<<"$OUT"; then verdict yes "release/br-alpha-0.1: br-alpha 0.1.1 only"; else verdict no "release/br-alpha-0.1: br-alpha 0.1.1 only"; fi
expect 1 'not a release line' "release/oops is not a publishing line" detect refs/heads/release/oops
expect 1 'not a publishing line' "a feature branch does not publish" detect refs/heads/feat/x

echo "== workflow_dispatch"
g checkout -q main
expect 0 '"version":"0.2.0"' "dispatch br-alpha 0.2.0 from main" detect refs/heads/main workflow_dispatch br-alpha 0.2.0
expect 1 'declares 0.2.0, not 0.9.0' "dispatch naming another version than the tree" detect refs/heads/main workflow_dispatch br-alpha 0.9.0
expect 1 'is not in TRACKED_LIBRARIES' "dispatch of an unknown crate" detect refs/heads/main workflow_dispatch br-nothing 0.1.0
expect 3 'may not publish' "dispatch refused by the law (0.1.1 on main)" detect refs/heads/main workflow_dispatch br-alpha 0.1.1
expect 1 'scaffold' "dispatch of a 0.0.0 scaffold" detect refs/heads/main workflow_dispatch br-delta 0.0.0

echo "== PR-time gate"
g checkout -q main
set_tracked "br-alpha br-beta br-delta"
commit_all "state" >/dev/null
g push -q origin main
g checkout -q -b feat/nothing
commit_all "no bump" >/dev/null
expect 0 'nothing to gate' "no version bump: nothing to gate" in_dev env BASE_REF=main bash .github/scripts/pr-registry-gate.sh
g checkout -q -b feat/bump main
set_version br-delta 0.1.0
commit_all "delta 0.1.0" >/dev/null
expect 1 'BR_REGISTRY_KEY is not set' "bump with no registry key (fork PR): fails loudly" in_dev env BASE_REF=main bash .github/scripts/pr-registry-gate.sh
stub_reset
eligible_target null true 0.1.0
expect 0 'eligible for release' "bump with an eligible target: passes" in_dev env BASE_REF=main BR_REGISTRY_KEY=k bash .github/scripts/pr-registry-gate.sh
stub_error librariesValidateReleaseTarget PATCH_NOT_SEALED "not sealed"
expect 1 'not sealed' "bump on an unsealed patch: fails" in_dev env BASE_REF=main BR_REGISTRY_KEY=k bash .github/scripts/pr-registry-gate.sh
stub_error librariesValidateReleaseTarget PATCH_IMPLEMENTED "implemented"
expect 1 'immutable' "bump on an implemented patch: already released, fails" in_dev env BASE_REF=main BR_REGISTRY_KEY=k bash .github/scripts/pr-registry-gate.sh
g checkout -q main
g branch -q -D feat/bump
g checkout -q -b feat/sibling main
set_version br-beta 0.1.0
commit_all "beta 0.1.0 with a bare path dependency on alpha" >/dev/null
stub_reset
eligible_target null true 0.1.0
expect 1 'br-beta depends on its tracked sibling br-alpha without a tag pin' "bump of a crate with a bare path dependency on a tracked sibling: fails" in_dev env BASE_REF=main BR_REGISTRY_KEY=k bash .github/scripts/pr-registry-gate.sh
g checkout -q main
g branch -q -D feat/sibling
g checkout -q -b feat/line main
set_version br-alpha 0.1.5
commit_all "alpha 0.1.5 into main" >/dev/null
expect 3 'may not publish' "bump into main inside a minor a line owns: refused by the law" in_dev env BASE_REF=main BR_REGISTRY_KEY=k bash .github/scripts/pr-registry-gate.sh

summary
