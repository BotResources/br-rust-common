#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
    echo "usage: $0 <crate> <version> <git-ref> <commit>" >&2
    exit 2
fi

crate="$1"
version="$2"
ref="$3"
commit="$4"
scripts="$(cd "$(dirname "$0")" && pwd)"
changelog="crates/${crate}/CHANGELOG.md"
tag="${crate}/v${version}"

if [ "$(bash "$scripts/crate-version.sh" "$crate" "$commit")" != "$version" ]; then
  echo "::error::${crate} at ${commit} does not declare ${version}" >&2
  exit 1
fi

if git rev-parse --verify "refs/tags/${tag}" >/dev/null 2>&1; then
  echo "✓ tag ${tag} already exists"
else
  echo "→ creating tag ${tag}"
  git tag -a "${tag}" "${commit}" \
    -m "${crate} ${version}" \
    -m "Auto-tagged from ${ref#refs/heads/}. See ${changelog} for the entry."
  git push origin "refs/tags/${tag}"
fi

if gh release view "${tag}" >/dev/null 2>&1; then
  echo "✓ release ${tag} already exists"
else
  echo "→ creating release ${tag}"
  notes_file=$(mktemp)
  git show "${commit}:${changelog}" | awk -v target="## ${version}" '
    !in_section && substr($0, 1, length(target)) == target &&
    (length($0) == length(target) || substr($0, length(target) + 1, 1) == " ") {
      in_section = 1; print; next
    }
    in_section && /^## / { exit }
    in_section { print }
  ' > "$notes_file"
  if [ ! -s "$notes_file" ]; then
    echo "::warning file=${changelog}::could not extract notes for ${tag}; using placeholder"
    printf 'See [%s](https://github.com/%s/blob/%s/%s) for changes.\n' \
      "$changelog" "$GITHUB_REPOSITORY" "$tag" "$changelog" > "$notes_file"
  fi
  latest=$(bash "$scripts/release-line.sh" may-move-latest "$ref")
  gh release create "${tag}" \
    --title "${crate} ${version}" \
    --notes-file "$notes_file" \
    --verify-tag \
    --latest="${latest}"
  rm -f "$notes_file"
fi
