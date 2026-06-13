#!/usr/bin/env bash

set -euo pipefail

today=$(date -u +%Y-%m-%d)
changelog="CHANGELOG.md"

version=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/^version = "([^"]+)".*/\1/')
if [ -z "$version" ]; then
  echo "::error file=Cargo.toml::could not extract [workspace.package] version" >&2
  exit 1
fi

if [ ! -f "$changelog" ]; then
  echo "::error file=Cargo.toml::workspace version ${version} but ${changelog} is missing" >&2
  exit 1
fi

line=$(grep -m1 -F -- "## [${version}]" "$changelog" || true)
if [ -z "$line" ]; then
  echo "::error file=${changelog}::workspace v${version} has no '## [${version}]' entry. Add a '## [${version}] — YYYY-MM-DD' section before merging." >&2
  exit 1
fi

date_part=$(printf '%s' "$line" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -1 || true)
if [ -z "$date_part" ]; then
  echo "::error file=${changelog}::v${version}: entry is missing a YYYY-MM-DD date. Expected '## [${version}] — YYYY-MM-DD'." >&2
  exit 1
fi

year=${date_part%%-*}
rest=${date_part#*-}
month=${rest%%-*}
day=${rest#*-}
if [ "$year" -lt 2020 ] || [ "$year" -gt 2099 ] \
    || [ "$month" -lt 1 ] || [ "$month" -gt 12 ] \
    || [ "$day" -lt 1 ] || [ "$day" -gt 31 ]; then
  echo "::error file=${changelog}::v${version}: date '${date_part}' has impossible components (expected YYYY-MM-DD with year 2020-2099)." >&2
  exit 1
fi

if [ "$date_part" \> "$today" ]; then
  echo "::error file=${changelog}::v${version}: date '${date_part}' is in the future (today UTC is ${today})." >&2
  exit 1
fi

echo "✓ workspace v${version} — ${date_part}"
