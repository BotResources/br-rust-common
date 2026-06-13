#!/usr/bin/env bash

set -euo pipefail
shopt -s nullglob

workspace_version=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/^version = "([^"]+)".*/\1/')
if [ -z "$workspace_version" ]; then
  echo "::error file=Cargo.toml::could not extract [workspace.package] version" >&2
  exit 1
fi
expected="v${workspace_version}"

fail=0

for readme in README.md crates/*/README.md; do
  [ -f "$readme" ] || continue

  while IFS= read -r raw; do
    [ -n "$raw" ] || continue
    if ! grep -qE '^tag = "v[0-9]+\.[0-9]+\.[0-9]+"$' <<<"$raw"; then
      echo "::error file=${readme}::malformed tag pin ${raw} — expected tag = \"vX.Y.Z\" (single repo tag, unified versioning)" >&2
      fail=1
      continue
    fi
    pinned=$(sed -E 's/^tag = "(.*)"$/\1/' <<<"$raw")
    if [ "$pinned" != "$expected" ]; then
      echo "::error file=${readme}::stale pin ${pinned}: the workspace is ${expected}. Update the tag in ${readme}." >&2
      fail=1
      continue
    fi
    echo "✓ ${readme}: pin ${pinned} matches the workspace version"
  done < <(grep -oE 'tag = "[^"]*"' "$readme" || true)
done

for toml in crates/*/Cargo.toml; do
  crate=$(basename "$(dirname "$toml")")
  readme="crates/${crate}/README.md"
  if [ ! -f "$readme" ]; then
    echo "::error file=${toml}::${crate} has no README.md" >&2
    fail=1
    continue
  fi
  if ! grep -qE "package = \"${crate}\"" "$readme"; then
    echo "○ ${crate}: not documented as a normal-dependency install, no tag pin required"
    continue
  fi
  if ! grep -qE "tag = \"${expected}\"" "$readme"; then
    echo "::error file=${readme}::${crate} README documents a 'package = \"${crate}\"' install but does not pin the workspace tag (tag = \"${expected}\")" >&2
    fail=1
  fi
done

exit $fail
