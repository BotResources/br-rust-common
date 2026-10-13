#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
SCRIPTS="$PWD/.github/scripts"
LAW="$SCRIPTS/release-line.sh"
source "$SCRIPTS/tracked-libraries.sh"
source "$SCRIPTS/library-meta.sh"

BASE_REF="${BASE_REF:-main}"

git fetch -q origin "$BASE_REF"

line="$("$LAW" check-ref "refs/heads/${BASE_REF}")"

refuse_untracked_releases

gated=0
for crate in ${TRACKED_LIBRARIES:-}; do
    if [[ ! -f "crates/${crate}/Cargo.toml" ]]; then
        if [[ "$line" == release/* ]]; then
            echo "${crate}: not in this ${line} tree — nothing to gate."
            continue
        fi
        echo "::error::missing crates/${crate}/Cargo.toml for a tracked crate" >&2
        exit 1
    fi

    version="$(bash "$SCRIPTS/crate-version.sh" "$crate")"
    base_version="$(bash "$SCRIPTS/crate-version.sh" "$crate" "origin/${BASE_REF}" 2>/dev/null || true)"

    if [[ "$version" == "$base_version" ]]; then
        continue
    fi
    if [[ "$version" == "0.0.0" ]]; then
        echo "${crate}: 0.0.0 scaffold — not gated."
        continue
    fi
    if non_release "$version"; then
        echo "${crate}: ${version} is non-release — not gated."
        continue
    fi

    echo "${crate}: ${base_version:-<absent>} -> ${version} — gating against ${line} and the registry."
    "$LAW" check "$crate" "$version" "refs/heads/${line}"
    bash "$SCRIPTS/registry-gate.sh" "$crate" "$version"
    gated=1
done

[[ "$gated" -eq 1 ]] || echo "No tracked crate version bump in this PR — nothing to gate."
