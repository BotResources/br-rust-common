#!/usr/bin/env bash
set -euo pipefail

SCRIPTS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/test-pins.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

URL="https://github.com/BotResources/br-rust-common"
failures=0
cases=0

pin() {
    printf '%s = { git = "%s", package = "%s", tag = "%s/v%s", version = "%s" }' "$1" "$URL" "$1" "$1" "$2" "$2"
}

fixture() {
    rm -rf "$WORK/repo"
    mkdir -p "$WORK/repo/crates/br-alpha" "$WORK/repo/crates/br-beta"
    printf '[package]\nname = "br-alpha"\nversion = "%s"\n' "$1" >"$WORK/repo/crates/br-alpha/Cargo.toml"
    printf '[package]\nname = "br-beta"\nversion = "0.0.0"\n' >"$WORK/repo/crates/br-beta/Cargo.toml"
}

expect() {
    local want_rc="$1" what="$2" crate="$3" rc=0 out
    cases=$((cases + 1))
    out="$(cd "$WORK/repo" && bash "$SCRIPTS/check-readme-pins.sh" "$crate" 2>&1)" || rc=$?
    if [[ "$rc" -eq "$want_rc" ]]; then
        printf '  ok  %s\n' "$what"
    else
        failures=$((failures + 1))
        printf 'FAIL  %s (exit %s)\n' "$what" "$rc"
        printf '%s\n' "$out" | sed 's/^/        | /' | head -6
    fi
}

echo "== README pin gate"
fixture 0.0.0
printf '# readme\n' >"$WORK/repo/README.md"
expect 0 "0.0.0 scaffold: nothing to check" br-alpha

fixture 0.2.0
expect 1 "released crate, no README: fails" br-alpha
printf '# readme\n' >"$WORK/repo/README.md"
expect 1 "released crate, no pin in the root README: fails" br-alpha
pin br-alpha 0.2.0 >"$WORK/repo/README.md"
expect 0 "released crate, exact pin: passes" br-alpha
pin br-alpha 0.1.0 >"$WORK/repo/README.md"
expect 1 "stale pin: fails" br-alpha
printf '%s\n%s\n' "$(pin br-alpha 0.2.0)" "$(pin br-alpha 0.1.0)" >"$WORK/repo/README.md"
expect 1 "exact pin plus a stale one: fails" br-alpha
printf '%s\n%s\n' "$(pin br-alpha 0.2.0)" "$(pin br-beta 0.9.0)" >"$WORK/repo/README.md"
expect 0 "another crate's pin is not this crate's concern" br-alpha

pin br-alpha 0.2.0 >"$WORK/repo/README.md"
printf 'br-alpha = { git = "%s", package = "br-alpha", tag = "v1.4.0", version = "1.4.0" }\n' "$URL" >"$WORK/repo/crates/br-alpha/README.md"
expect 1 "crate README still on the workspace tag: fails" br-alpha
printf 'br-alpha = { git = "%s", package = "br-alpha", tag = "br-alpha/v0.2.0", version = "0.2.0", features = ["x"] }\n' "$URL" >"$WORK/repo/crates/br-alpha/README.md"
expect 0 "crate README pin with features on the right tag: passes" br-alpha

echo
if [[ "$failures" -gt 0 ]]; then
    echo "${failures}/${cases} case(s) failed" >&2
    exit 1
fi
echo "${cases} cases passed"
