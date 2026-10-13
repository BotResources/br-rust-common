#!/usr/bin/env bash
set -euo pipefail

SRC_SCRIPTS="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
for tool in git python3 cargo; do
    command -v "$tool" >/dev/null || { echo "${tool} is required" >&2; exit 2; }
done
REAL_CARGO="$(command -v cargo)"
export REAL_CARGO

WORK="$(mktemp -d "${TMPDIR:-/tmp}/test-release.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=scenario GIT_AUTHOR_EMAIL=scenario@example.invalid
export GIT_COMMITTER_NAME=scenario GIT_COMMITTER_EMAIL=scenario@example.invalid
unset GITHUB_OUTPUT GITHUB_REF GITHUB_REF_NAME GITHUB_SHA GITHUB_REPOSITORY GITHUB_BASE_REF \
    EVENT_NAME INPUT_CRATE INPUT_VERSION BASE_REF GATE_REV RELEASE_BRANCHES \
    BR_REGISTRY_KEY BR_REGISTRY_URL REGISTRY_WORK

ORIGIN="$WORK/origin.git"
DEV="$WORK/dev"
STUB_DIR="$WORK/stubs"
export STUB_DIR

ALPHA_ID="01a00000-0000-7000-8000-000000000001"
BETA_ID="01a00000-0000-7000-8000-000000000002"
DELTA_ID="01a00000-0000-7000-8000-000000000003"
export EXTERNAL_ID="01a00000-0000-7000-8000-000000000009"

failures=0
cases=0
OUT=""
RC=0

g() { git -C "$DEV" "$@"; }

run() {
    RC=0
    OUT="$("$@" 2>&1)" || RC=$?
}

verdict() {
    local ok="$1" what="$2"
    cases=$((cases + 1))
    if [[ "$ok" == yes ]]; then
        printf '  ok  %s\n' "$what"
    else
        failures=$((failures + 1))
        printf 'FAIL  %s (exit %s)\n' "$what" "$RC"
        printf '%s\n' "$OUT" | sed 's/^/        | /' | head -12
    fi
}

expect() {
    local want_rc="$1" needle="$2" what="$3"
    shift 3
    run "$@"
    if [[ "$RC" -eq "$want_rc" ]] && { [[ -z "$needle" ]] || grep -qF -- "$needle" <<<"$OUT"; }; then
        verdict yes "$what"
    else
        verdict no "$what"
    fi
}

expect_equal() {
    if [[ "$1" == "$2" ]]; then verdict yes "$3"; else OUT="got: $1 / want: $2"; verdict no "$3"; fi
}

expect_file_has() {
    if grep -qF -- "$2" "$1" 2>/dev/null; then verdict yes "$3"; else OUT="$(cat "$1" 2>/dev/null || true)"; verdict no "$3"; fi
}

expect_file_lacks() {
    if grep -qF -- "$2" "$1" 2>/dev/null; then OUT="$(cat "$1")"; verdict no "$3"; else verdict yes "$3"; fi
}

summary() {
    echo
    if [[ "$failures" -gt 0 ]]; then
        echo "${failures}/${cases} case(s) failed" >&2
        exit 1
    fi
    echo "${cases} cases passed"
}

install_stubs() {
    mkdir -p "$WORK/bin" "$STUB_DIR/requests"
    cat >"$WORK/bin/curl" <<'STUB'
#!/usr/bin/env bash
out=""
data=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift ;;
        --data-binary) data="${2#@}"; shift ;;
    esac
    shift
done
n=$(( $(ls "$STUB_DIR/requests" | wc -l) + 1 ))
cp "$data" "$STUB_DIR/requests/$n.json"
field=$(grep -o 'libraries[A-Za-z]*(' "$data" | head -1 | tr -d '(')
if [ -f "$STUB_DIR/resp-$field.json" ]; then
    cp "$STUB_DIR/resp-$field.json" "$out"
else
    printf '{"data":{"%s":{"success":true,"warnings":[]}}}' "$field" >"$out"
fi
cat "$STUB_DIR/status-$field" 2>/dev/null || echo 200
STUB
    cat >"$WORK/bin/gh" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "gh $*" >>"$STUB_DIR/gh.log"
[ "${1:-}" = release ] && [ "${2:-}" = view ] && exit 1
exit 0
STUB
    cat >"$WORK/bin/cargo" <<'STUB'
#!/usr/bin/env bash
if [ "${1:-}" = public-api ]; then
    printf '%s\n' "$*" >>"$STUB_DIR/cargo.log"
    if printf ' %s ' "$@" | grep -q ' diff '; then
        printf 'DIFF-OUTPUT\n'
    else
        printf 'pub fn listing()\n'
    fi
    exit 0
fi
exec "$REAL_CARGO" "$@"
STUB
    chmod +x "$WORK/bin/curl" "$WORK/bin/gh" "$WORK/bin/cargo"
    export PATH="$WORK/bin:$PATH"
}

stub_reset() {
    rm -rf "$STUB_DIR"
    mkdir -p "$STUB_DIR/requests"
}

stub_response() {
    printf '%s' "$2" >"$STUB_DIR/resp-$1.json"
}

stub_error() {
    stub_response "$1" "{\"errors\":[{\"message\":\"$3\",\"extensions\":{\"reason\":\"$2\"}}],\"data\":null}"
}

eligible_target() {
    stub_response librariesValidateReleaseTarget "{\"data\":{\"librariesValidateReleaseTarget\":{\"baselineTag\":${1:-null},\"baselineVersion\":null,\"libraryId\":\"$ALPHA_ID\",\"line\":\"0.1\",\"opensLine\":${2:-true},\"version\":\"$3\"}}}"
}

write_crate() {
    local dir="$DEV/crates/$1"
    mkdir -p "$dir/src"
    {
        printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2024"\n' "$1"
        printf '%b' "${2:-}"
    } >"$dir/Cargo.toml"
    echo "pub fn x() {}" >"$dir/src/lib.rs"
    printf '# Changelog\n\n## Unreleased\n' >"$dir/CHANGELOG.md"
}

make_fixture() {
    git init -q --bare -b main "$ORIGIN"
    git clone -q "$ORIGIN" "$DEV" 2>/dev/null
    g checkout -q -b main
    mkdir -p "$DEV/.github" "$DEV/vendor/extdep/src"
    cp -R "$SRC_SCRIPTS" "$DEV/.github/scripts"
    printf '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\nexclude = ["vendor"]\n' >"$DEV/Cargo.toml"
    printf '[package]\nname = "extdep"\nversion = "2.3.4"\nedition = "2024"\n' >"$DEV/vendor/extdep/Cargo.toml"
    echo "pub fn e() {}" >"$DEV/vendor/extdep/src/lib.rs"
    write_crate br-alpha
    write_crate br-delta
    write_crate br-beta '\n[dependencies]\nbr-alpha = { path = "../br-alpha" }\nextdep = { path = "../../vendor/extdep" }\n\n[dev-dependencies]\nbr-delta = { path = "../br-delta" }\n'
    printf 'library-id = "%s"\n' "$ALPHA_ID" >"$DEV/crates/br-alpha/registry.toml"
    printf 'library-id = "%s"\n' "$BETA_ID" >"$DEV/crates/br-beta/registry.toml"
    printf 'library-id = "%s"\n' "$DELTA_ID" >"$DEV/crates/br-delta/registry.toml"
    printf 'target\n' >"$DEV/.gitignore"
    (cd "$DEV" && cargo generate-lockfile --offline -q)
    g add -A && g commit -q -m "fixture" && g push -q origin main
}

set_tracked() {
    printf '#!/usr/bin/env bash\nexport TRACKED_LIBRARIES="%s"\n' "$1" >"$DEV/.github/scripts/tracked-libraries.sh"
}

set_version() {
    local f="$DEV/crates/$1/Cargo.toml"
    sed -e "s/^version = \".*\"/version = \"$2\"/" "$f" >"$f.new" && mv "$f.new" "$f"
    printf '\n## %s\n\n- change\n' "$2" >>"$DEV/crates/$1/CHANGELOG.md"
    (cd "$DEV" && cargo generate-lockfile --offline -q)
}

commit_all() {
    g add -A && g commit -q -m "$1" --allow-empty && g rev-parse HEAD
}

in_dev() {
    (cd "$DEV" && "$@")
}
