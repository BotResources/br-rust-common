#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/test-lib.sh"

make_fixture
install_stubs
set_tracked "br-alpha br-beta br-delta"
set_version br-alpha 0.1.0
set_version br-beta 0.1.0
RELEASE_COMMIT="$(commit_all "alpha and beta 0.1.0")"
g push -q origin main

gate() { in_dev env BR_REGISTRY_KEY=k GITHUB_OUTPUT="$WORK/gate.out" bash .github/scripts/registry-gate.sh "$@"; }
gate_nokey() { in_dev bash .github/scripts/registry-gate.sh "$@"; }
record() { in_dev env BR_REGISTRY_KEY=k REGISTRY_WORK="$WORK/reg" bash .github/scripts/registry-record.sh "$@"; }
requests() { ls "$STUB_DIR/requests" | wc -l | tr -d ' '; }
request_field() { python3 -c '
import json, sys
d = json.load(open(sys.argv[1]))
v = d["variables"]
for key in sys.argv[2].split("."):
    v = v[int(key)] if key.isdigit() else v[key]
print(json.dumps(v) if not isinstance(v, str) else v)
' "$STUB_DIR/requests/$1.json" "$2"; }

echo "== registry gate"
stub_reset
expect 1 'BR_REGISTRY_KEY is not set' "no registry key: refuses" gate_nokey br-alpha 0.1.0
eligible_target '"br-alpha/v0.0.9"' false 0.1.0
: >"$WORK/gate.out"
expect 0 'eligible for release' "eligible target" gate br-alpha 0.1.0
expect_file_has "$WORK/gate.out" 'baseline_tag=br-alpha/v0.0.9' "baseline tag written to GITHUB_OUTPUT"
expect_file_has "$WORK/gate.out" 'opens_line=false' "opensLine written to GITHUB_OUTPUT"
expect_file_has "$WORK/gate.out" 'implemented=false' "implemented=false written"
expect_file_has "$STUB_DIR/requests/1.json" "$ALPHA_ID" "the committed library id is the one queried"
expect 1 'declares 0.1.0 in Cargo.toml' "version differing from Cargo.toml: refuses" gate br-alpha 0.2.0
expect 1 'not in TRACKED_LIBRARIES' "untracked crate: refuses" gate br-nothing 0.1.0
for reason in FORBIDDEN NOT_FOUND VERSION_INVALID PATCH_NOT_FOUND PATCH_NOT_SEALED LINE_RETIRED; do
    stub_error librariesValidateReleaseTarget "$reason" "m"
    expect 1 "($reason)" "refusal $reason maps to a failure naming it" gate br-alpha 0.1.0
done
stub_error librariesValidateReleaseTarget PATCH_IMPLEMENTED "m"
expect 1 'immutable' "PATCH_IMPLEMENTED without a tag argument: fails" gate br-alpha 0.1.0
expect 1 'does not exist in this repository' "PATCH_IMPLEMENTED, tag argument but no tag: fails" gate br-alpha 0.1.0 br-alpha/v0.1.0
g tag -a br-alpha/v0.1.0 -m t "$RELEASE_COMMIT"
: >"$WORK/gate.out"
expect 0 'idempotent re-run' "PATCH_IMPLEMENTED with the tag present: exit 0" gate br-alpha 0.1.0 br-alpha/v0.1.0
expect_file_has "$WORK/gate.out" 'implemented=true' "implemented=true written"
g tag -d br-alpha/v0.1.0 >/dev/null
printf '<html>' >"$STUB_DIR/resp-librariesValidateReleaseTarget.json"
printf '502' >"$STUB_DIR/status-librariesValidateReleaseTarget"
expect 1 'no eligible verdict' "gateway error: fails" gate br-alpha 0.1.0

echo "== committed library id"
stub_reset
eligible_target null true 0.1.0
mv "$DEV/crates/br-alpha/registry.toml" "$WORK/alpha.toml"
expect 1 'not found' "missing registry.toml: refuses" gate br-alpha 0.1.0
printf 'library-id = "not-a-uuid"\n' >"$DEV/crates/br-alpha/registry.toml"
expect 1 'well-formed' "malformed id: refuses" gate br-alpha 0.1.0
printf 'library-id = "%s"\nextra = "x"\n' "$ALPHA_ID" >"$DEV/crates/br-alpha/registry.toml"
expect 1 'exactly one key' "a second key: refuses" gate br-alpha 0.1.0
mv "$WORK/alpha.toml" "$DEV/crates/br-alpha/registry.toml"

echo "== tag and release"
gh_log="$STUB_DIR/gh.log"
expect 0 'creating tag br-alpha/v0.1.0' "tag br-alpha/v0.1.0 created" in_dev env GITHUB_REPOSITORY=o/r bash .github/scripts/release-crate.sh br-alpha 0.1.0 refs/heads/main "$RELEASE_COMMIT"
expect_file_has "$gh_log" '--latest=true' "main moves Latest"
expect 0 '' "tag pushed to origin" git -C "$ORIGIN" rev-parse -q --verify refs/tags/br-alpha/v0.1.0
expect 0 'already exists' "second run: tag exists, nothing recreated" in_dev env GITHUB_REPOSITORY=o/r bash .github/scripts/release-crate.sh br-alpha 0.1.0 refs/heads/main "$RELEASE_COMMIT"
expect 0 'creating tag br-beta/v0.1.0' "tag br-beta/v0.1.0 from a release line" in_dev env GITHUB_REPOSITORY=o/r bash .github/scripts/release-crate.sh br-beta 0.1.0 refs/heads/release/br-beta-0.1 "$RELEASE_COMMIT"
expect_file_has "$gh_log" '--latest=false' "a release line never moves Latest"
expect 1 'does not declare' "tagging a version the commit does not carry: refuses" in_dev env GITHUB_REPOSITORY=o/r bash .github/scripts/release-crate.sh br-alpha 0.9.0 refs/heads/main "$RELEASE_COMMIT"

echo "== record: no baseline, no dependencies"
stub_reset
expect 0 'br-alpha 0.1.0 recorded' "br-alpha recorded" record br-alpha 0.1.0 br-alpha/v0.1.0 "$RELEASE_COMMIT"
expect_equal "$(requests)" 3 "three records sent"
expect_equal "$(request_field 1 input.generated)" true "listing is marked generated"
expect_equal "$(request_field 1 input.apiListing)" "pub fn listing()" "listing is the cargo public-api output"
expect_equal "$(request_field 1 input.apiDiff)" "" "no baseline: empty diff"
expect_equal "$(request_field 2 input.dependencies)" "[]" "no registered dependency: empty set"
expect_equal "$(request_field 3 input.releaseTag)" "br-alpha/v0.1.0" "release tag recorded"
expect_equal "$(request_field 3 input.releaseCommit)" "$RELEASE_COMMIT" "release commit recorded"

echo "== record: baseline, workspace and registered dependencies"
stub_reset
rm -rf "$WORK/reg"
g tag -a br-beta/v0.0.9 -m old "$(g rev-parse HEAD~1)" 2>/dev/null || g tag -a br-beta/v0.0.9 -m old "$RELEASE_COMMIT"
stub_response librariesLookupPackages "{\"data\":{\"librariesLookupPackages\":[{\"found\":true,\"packageName\":\"extdep\",\"version\":\"2.3.4\",\"line\":\"2.3\",\"lineStatus\":\"ACTIVE\",\"library\":{\"id\":\"$EXTERNAL_ID\",\"name\":\"extdep\"}}]}}"
stub_response librariesRecordDependencies '{"data":{"librariesRecordDependencies":{"success":true,"warnings":[{"code":"DEPENDENCY_LINE_DEPRECATED","params":{"x":1}}]}}}'
expect 0 '::warning::registry: DEPENDENCY_LINE_DEPRECATED' "warnings printed as ::warning::" record br-beta 0.1.0 br-beta/v0.1.0 "$RELEASE_COMMIT" br-beta/v0.0.9
expect_equal "$(requests)" 4 "look-up plus three records"
expect_file_has "$STUB_DIR/requests/1.json" '"packageName": "extdep"' "only the package outside the workspace is looked up"
expect_file_lacks "$STUB_DIR/requests/1.json" 'br-alpha' "workspace crates are not looked up"
expect_equal "$(request_field 3 input.dependencies)" "[{\"libraryId\": \"$ALPHA_ID\", \"version\": \"0.1.0\"}, {\"libraryId\": \"$EXTERNAL_ID\", \"version\": \"2.3.4\"}]" "dependencies: br-alpha (workspace) and extdep (look-up), no dev-dependency"
expect_equal "$(request_field 2 input.apiDiff)" "DIFF-OUTPUT" "baseline: diff recorded verbatim"
expect_file_has "$STUB_DIR/cargo.log" "diff br-beta/v0.0.9..$RELEASE_COMMIT" "diff is taken from the baseline tag to the release commit"

echo "== record: refusals and recovery"
stub_reset
rm -rf "$WORK/reg"
stub_error librariesRecordApiListing API_LISTING_TOO_LARGE "too big"
expect 1 'API_LISTING_TOO_LARGE' "listing too large: surfaced clearly" record br-alpha 0.1.0 br-alpha/v0.1.0 "$RELEASE_COMMIT"
stub_reset
rm -rf "$WORK/reg"
stub_error librariesRecordRelease PATCH_NOT_SEALED "x"
expect 1 'PATCH_NOT_SEALED' "release record refused: fails after the first two records" record br-alpha 0.1.0 br-alpha/v0.1.0 "$RELEASE_COMMIT"
stub_reset
rm -rf "$WORK/reg"
stub_error librariesRecordApiListing PATCH_IMPLEMENTED "x"
expect 0 'already implemented' "PATCH_IMPLEMENTED during record: complete, exit 0" record br-alpha 0.1.0 br-alpha/v0.1.0 "$RELEASE_COMMIT"
OTHER_COMMIT="$(commit_all "another commit carrying the same version")"
expect 1 'does not point at' "tag on another commit: refuses" record br-alpha 0.1.0 br-alpha/v0.1.0 "$OTHER_COMMIT"
expect 1 'must be br-alpha/v0.1.0' "tag in the wrong format: refuses" record br-alpha 0.1.0 br-alpha-v0.1.0 "$RELEASE_COMMIT"
expect 1 'BR_REGISTRY_KEY is not set' "no key: refuses" in_dev bash .github/scripts/registry-record.sh br-alpha 0.1.0 br-alpha/v0.1.0 "$RELEASE_COMMIT"

echo "== record: sibling pinned by tag and patched to the local path"
stub_reset
rm -rf "$WORK/reg"
GAMMA_ID="01a00000-0000-7000-8000-00000000000a"
write_crate br-gamma "\n[dependencies]\nbr-alpha = { git = \"file://$ORIGIN\", tag = \"br-alpha/v0.1.0\", version = \"0.1.0\" }\n"
printf 'library-id = "%s"\n' "$GAMMA_ID" >"$DEV/crates/br-gamma/registry.toml"
printf '\n[patch."file://%s"]\nbr-alpha = { path = "crates/br-alpha" }\n' "$ORIGIN" >>"$DEV/Cargo.toml"
set_tracked "br-alpha br-beta br-delta br-gamma"
sed -e 's/^version = "0.1.0"/version = "0.1.1"/' "$DEV/crates/br-alpha/Cargo.toml" >"$DEV/crates/br-alpha/Cargo.toml.new" && mv "$DEV/crates/br-alpha/Cargo.toml.new" "$DEV/crates/br-alpha/Cargo.toml"
sed -e 's/^version = "0.0.0"/version = "0.1.0"/' "$DEV/crates/br-gamma/Cargo.toml" >"$DEV/crates/br-gamma/Cargo.toml.new" && mv "$DEV/crates/br-gamma/Cargo.toml.new" "$DEV/crates/br-gamma/Cargo.toml"
printf '\n## 0.1.0\n\n- change\n' >>"$DEV/crates/br-gamma/CHANGELOG.md"
(cd "$DEV" && cargo generate-lockfile -q)
GAMMA_COMMIT="$(commit_all "gamma 0.1.0 pins alpha 0.1.0 by tag while alpha is already 0.1.1 locally")"
g tag -a br-gamma/v0.1.0 -m t "$GAMMA_COMMIT"
expect 0 'br-gamma 0.1.0 recorded' "br-gamma recorded" record br-gamma 0.1.0 br-gamma/v0.1.0 "$GAMMA_COMMIT"
expect_equal "$(requests)" 3 "no look-up: the sibling is a registered library, not an external package"
expect_equal "$(request_field 2 input.dependencies)" "[{\"libraryId\": \"$ALPHA_ID\", \"version\": \"0.1.0\"}]" "the sibling is recorded at the tag its pin names, not at its local version"

summary
