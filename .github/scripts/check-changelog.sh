#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: check-changelog.sh <crate> [<rev>]}"
rev="${2:-}"
version=$(bash "$(dirname "$0")/crate-version.sh" "$crate" "$rev")
changelog="crates/${crate}/CHANGELOG.md"

if [ -n "$rev" ]; then
  content=$(git show "${rev}:${changelog}" 2>/dev/null) || content=""
  present=$([ -n "$content" ] && echo yes || echo no)
else
  present=$([ -f "$changelog" ] && echo yes || echo no)
  content=$([ "$present" = yes ] && cat "$changelog" || true)
fi

if [ "$present" = no ]; then
  echo "::error::${crate} version ${version} but ${changelog} is missing" >&2
  exit 1
fi

if [ "$version" = "0.0.0" ]; then
  if grep -qE "^## 0\.0\.0( |\$)" <<<"$content"; then
    echo "::error file=${changelog}::0.0.0 is the unreleased scaffold version and must not carry a release heading" >&2
    exit 1
  fi
  echo "✓ ${crate} 0.0.0 scaffold — unreleased, nothing to tag"
  exit 0
fi

if ! grep -qE "^## ${version}( |\$)" <<<"$content"; then
  echo "::error file=${changelog}::${crate}/v${version} has no '## ${version}' entry. Add a plain '## ${version}' section before merging." >&2
  exit 1
fi

echo "✓ ${crate}/v${version}"
