#!/usr/bin/env bash
# For every crates/*/Cargo.toml, fail if the declared version is missing from
# the matching CHANGELOG.md or lacks a sane date. The release-tags workflow
# publishes <crate>-v<version> tags directly from Cargo.toml and creates a
# GitHub Release whose body comes from the matching CHANGELOG section, so a
# missing or malformed entry would land an undocumented release.
#
# Required header shape (per README):
#   ## [X.Y.Z] — YYYY-MM-DD
#
# Enforced checks per crate:
#   (a) `## [X.Y.Z]` section exists in CHANGELOG.md
#   (b) the section header line contains a YYYY-MM-DD date
#   (c) the date components are plausible (year 2020-2099, month 1-12, day 1-31)
#   (d) the date is not in the future (UTC)
#
# Used by:
#   - .github/workflows/ci.yml          (pre-merge PR gate)
#   - .github/workflows/release-tags.yml (pre-tag safety net)

set -euo pipefail
shopt -s nullglob

today=$(date -u +%Y-%m-%d)
fail=0

for toml in crates/*/Cargo.toml; do
  crate=$(basename "$(dirname "$toml")")
  changelog="crates/${crate}/CHANGELOG.md"

  version=$(grep -m1 '^version = ' "$toml" | sed -E 's/^version = "([^"]+)".*/\1/')
  if [ -z "$version" ]; then
    echo "::error file=${toml}::could not extract version" >&2
    fail=1
    continue
  fi

  if [ ! -f "$changelog" ]; then
    echo "::error file=${toml}::${crate} has version ${version} but ${changelog} is missing" >&2
    fail=1
    continue
  fi

  # (a) section header exists
  line=$(grep -m1 -F -- "## [${version}]" "$changelog" || true)
  if [ -z "$line" ]; then
    echo "::error file=${changelog}::${crate} v${version} has no '## [${version}]' entry. Add a '## [${version}] — YYYY-MM-DD' section before merging." >&2
    fail=1
    continue
  fi

  # (b) header line contains a YYYY-MM-DD date
  date_part=$(printf '%s' "$line" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -1 || true)
  if [ -z "$date_part" ]; then
    echo "::error file=${changelog}::${crate} v${version}: entry is missing a YYYY-MM-DD date. Expected '## [${version}] — YYYY-MM-DD'." >&2
    fail=1
    continue
  fi

  # (c) components plausible
  year=${date_part%%-*}
  rest=${date_part#*-}
  month=${rest%%-*}
  day=${rest#*-}
  if [ "$year" -lt 2020 ] || [ "$year" -gt 2099 ] \
      || [ "$month" -lt 1 ] || [ "$month" -gt 12 ] \
      || [ "$day" -lt 1 ] || [ "$day" -gt 31 ]; then
    echo "::error file=${changelog}::${crate} v${version}: date '${date_part}' has impossible components (expected YYYY-MM-DD with year 2020-2099)." >&2
    fail=1
    continue
  fi

  # (d) not in the future
  if [ "$date_part" \> "$today" ]; then
    echo "::error file=${changelog}::${crate} v${version}: date '${date_part}' is in the future (today UTC is ${today})." >&2
    fail=1
    continue
  fi

  echo "✓ ${crate} v${version} — ${date_part}"
done

exit $fail
