#!/usr/bin/env bash
LIBRARY_META_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIBRARY_UUID_RE='^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'

is_tracked() {
    case " ${TRACKED_LIBRARIES:-} " in
        *" $1 "*) return 0 ;;
        *) return 1 ;;
    esac
}

library_ids() {
    local crate="$1" want_version="${2:-}" rev="${3:-}"
    local file="crates/${crate}/registry.toml" declared

    if ! is_tracked "$crate"; then
        echo "::error::library ids: ${crate} is not in TRACKED_LIBRARIES (.github/scripts/tracked-libraries.sh) — refusing to touch the registry for it." >&2
        return 1
    fi

    if [[ ! -f "$file" ]]; then
        echo "::error::library ids: ${file} not found. Every tracked crate commits its library id: library-id = \"<uuid>\"." >&2
        return 1
    fi

    local stray
    stray=$(grep -vE '^[[:space:]]*(library-id[[:space:]]*=[[:space:]]*"[^"]*"[[:space:]]*)?$' "$file" || true)
    if [[ -n "$stray" ]]; then
        echo "::error::library ids: ${file} must hold exactly one key, library-id; found: ${stray%%$'\n'*}" >&2
        return 1
    fi

    LIB_REG_LIBRARY_ID=$(sed -n 's/^[[:space:]]*library-id[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$file" | head -1)
    if [[ ! "$LIB_REG_LIBRARY_ID" =~ $LIBRARY_UUID_RE ]]; then
        echo "::error::library ids: ${file} has no well-formed library-id (got '${LIB_REG_LIBRARY_ID}')." >&2
        return 1
    fi

    if [[ -n "$want_version" ]]; then
        declared=$(bash "$LIBRARY_META_DIR/crate-version.sh" "$crate" "$rev") || return 1
        if [[ "$declared" != "$want_version" ]]; then
            echo "::error::library ids: asked to act on ${crate} ${want_version}, but ${rev:-this checkout} declares ${declared} in Cargo.toml. The API listing and the dependencies are read from that tree, so proceeding would describe ${declared} inside ${want_version}'s registry patch." >&2
            return 1
        fi
    fi
}

non_release() {
    [[ "$1" == *-dev* || "$1" == *-rc* || "$1" == *-pre* ]]
}

is_unpublished() {
    grep -qE '^publish[[:space:]]*=[[:space:]]*false[[:space:]]*$' "crates/$1/Cargo.toml"
}

refuse_untracked_releases() {
    local crate version failed=0
    for crate in $(bash "$LIBRARY_META_DIR/workspace-crates.sh"); do
        is_tracked "$crate" && continue
        is_unpublished "$crate" && continue
        version="$(bash "$LIBRARY_META_DIR/crate-version.sh" "$crate")"
        if [[ "$version" == "0.0.0" ]] || non_release "$version"; then
            continue
        fi
        echo "::error::${crate} declares ${version} but is not in TRACKED_LIBRARIES: a release line cannot publish a crate the registry does not track. Register its Library, commit crates/${crate}/registry.toml and add the crate to .github/scripts/tracked-libraries.sh." >&2
        failed=1
    done
    return "$failed"
}
