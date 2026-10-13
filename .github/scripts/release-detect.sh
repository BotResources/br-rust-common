#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
SCRIPTS="$PWD/.github/scripts"
LAW="$SCRIPTS/release-line.sh"
source "$SCRIPTS/tracked-libraries.sh"
source "$SCRIPTS/library-meta.sh"

: "${GITHUB_REF:?GITHUB_REF is required}"
EVENT_NAME="${EVENT_NAME:-push}"
HEAD_SHA="${GITHUB_SHA:-$(git rev-parse HEAD)}"

crate_version() {
    bash "$SCRIPTS/crate-version.sh" "$@"
}

tag_exists() {
    git rev-parse -q --verify "refs/tags/$1" >/dev/null \
        || git ls-remote --exit-code --tags origin "$1" >/dev/null 2>&1
}

require_tracked() {
    if ! is_tracked "$1"; then
        echo "::error::$1 is not in TRACKED_LIBRARIES (.github/scripts/tracked-libraries.sh) — refusing to release it." >&2
        return 1
    fi
}

explicit_entry() {
    local crate="$1" version="$2" branch tag commit declared
    require_tracked "$crate" || return 1
    branch="$("$LAW" check-ref "$GITHUB_REF")" || return 1
    "$LAW" check "$crate" "$version" "refs/heads/${branch}" >&2 || return $?
    tag="${crate}/v${version}"
    if tag_exists "$tag"; then
        git fetch -q origin "refs/tags/${tag}:refs/tags/${tag}" 2>/dev/null || true
        commit="$(git rev-list -n 1 "refs/tags/${tag}")" || return 1
        if ! git merge-base --is-ancestor "$commit" "$HEAD_SHA"; then
            echo "::error::${tag} points at ${commit}, which is not on ${branch}; run this workflow from the line that carries it." >&2
            return 1
        fi
    else
        commit="$HEAD_SHA"
    fi
    declared="$(crate_version "$crate" "$commit")" || return 1
    if [[ "$declared" != "$version" ]]; then
        echo "::error::${crate} at ${commit} declares ${declared}, not ${version}: releasing it would publish that code as ${version}." >&2
        return 1
    fi
    if [[ "$declared" == "0.0.0" ]]; then
        echo "::error::${crate} 0.0.0 is the unreleased scaffold version; there is nothing to release." >&2
        return 1
    fi
    printf '%s %s %s %s\n' "$crate" "$version" "$branch" "$commit"
}

ENTRIES=()
REFUSED=()

if [[ "$EVENT_NAME" == "workflow_dispatch" ]]; then
    ENTRIES+=("$(explicit_entry "${INPUT_CRATE:-}" "${INPUT_VERSION:-}")")
    echo "workflow_dispatch: ${ENTRIES[0]}"
else
    BRANCH="$("$LAW" check-ref "$GITHUB_REF")"
    refuse_untracked_releases
    CANDIDATES="${TRACKED_LIBRARIES:-}"
    if [[ "$BRANCH" == release/* ]]; then
        read -r LINE_CRATE _ _ <<<"$("$LAW" parse "$BRANCH")"
        require_tracked "$LINE_CRATE"
        CANDIDATES="$LINE_CRATE"
        echo "${BRANCH}: the release line of ${LINE_CRATE} — no other crate publishes here."
    fi

    for crate in $CANDIDATES; do
        if [[ ! -f "crates/${crate}/Cargo.toml" ]]; then
            if [[ "$BRANCH" == release/* ]]; then
                echo "::error::${BRANCH} names ${crate}, which crates/ does not carry." >&2
                exit 1
            fi
            echo "::error::missing crates/${crate}/Cargo.toml for a tracked crate" >&2
            exit 1
        fi

        version="$(crate_version "$crate")"
        if [[ "$version" == "0.0.0" ]]; then
            echo "${crate}: 0.0.0 scaffold — nothing to release."
            continue
        fi
        if non_release "$version"; then
            echo "${crate}: version ${version} is non-release — skipping."
            continue
        fi

        parent_version="$(crate_version "$crate" "${HEAD_SHA}^" 2>/dev/null || true)"
        if tag_exists "${crate}/v${version}" && [[ "$version" == "$parent_version" ]]; then
            echo "${crate}: ${crate}/v${version} already exists and the version did not change — already shipped."
            continue
        fi

        rc=0
        verdict="$("$LAW" check "$crate" "$version" "refs/heads/${BRANCH}" 2>&1)" || rc=$?
        if [[ "$rc" -eq 3 ]]; then
            printf '%s\n' "$verdict" >&2
            REFUSED+=("${crate} ${version}: $(sed -n 's/^::error::release-line: //p' <<<"$verdict" | head -n 1)")
            echo "${crate} ${version}: refused by the release-line law — not released; the rest of the scan goes on."
            continue
        fi
        if [[ "$rc" -ne 0 ]]; then
            printf '%s\n' "$verdict" >&2
            exit "$rc"
        fi
        echo "$verdict"

        ENTRIES+=("${crate} ${version} ${BRANCH} ${HEAD_SHA}")
        echo "Will release: ${crate} ${version} (line ${BRANCH})"
    done
fi

MATRIX="$(python3 -c '
import json, sys
keys = ("crate", "version", "line", "commit")
print(json.dumps([dict(zip(keys, line.split(" "))) for line in sys.argv[1:]], separators=(",", ":")))
' ${ENTRIES[@]+"${ENTRIES[@]}"})"
REFUSED_JSON="$(python3 -c '
import json, sys
print(json.dumps(sys.argv[1:], separators=(",", ":")))
' ${REFUSED[@]+"${REFUSED[@]}"})"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    {
        echo "matrix=${MATRIX}"
        echo "refused=${REFUSED_JSON}"
    } >>"$GITHUB_OUTPUT"
else
    echo "matrix=${MATRIX}"
    echo "refused=${REFUSED_JSON}"
fi
if [[ ${#ENTRIES[@]} -eq 0 ]]; then
    echo "No crates to release."
else
    echo "Release matrix: ${MATRIX}"
fi
if [[ ${#REFUSED[@]} -gt 0 ]]; then
    echo "::warning::${#REFUSED[@]} version(s) refused by the release-line law: ${REFUSED_JSON}"
fi
