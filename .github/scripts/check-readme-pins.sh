#!/usr/bin/env bash
# For every README (the root one and each crates/*/README.md), fail if a
# git-tag pin of the form `tag = "<crate>-vX.Y.Z"` names a version that does
# not match that crate's current Cargo.toml version.
#
# Why this gate exists: install snippets pin an *exact* tag by design — this
# is GitOps, downstream consumers must not float on a moving ref. The cost of
# that choice is that every version bump has to update the snippet in lockstep,
# which is easy to forget. This gate makes a stale pin impossible to merge
# instead of a recurring manual chore: the README always documents the version
# the source tree actually is.
#
# Required pin shape (per README):
#   tag = "<crate>-vX.Y.Z"
#
# Used by:
#   - .github/workflows/ci.yml (pre-merge PR gate, in the changelog job)

set -euo pipefail
shopt -s nullglob

fail=0

# Scan the root README and every crate README for tag pins.
for readme in README.md crates/*/README.md; do
  [ -f "$readme" ] || continue

  # Each pin line looks like:  tag = "<crate>-v1.2.3"
  while IFS= read -r match; do
    [ -n "$match" ] || continue
    # match = `<crate>-v<X.Y.Z>` — split on the last `-v`.
    crate=${match%-v*}
    pinned=${match##*-v}

    toml="crates/${crate}/Cargo.toml"
    if [ ! -f "$toml" ]; then
      echo "::error file=${readme}::pin references unknown crate '${crate}' (no ${toml})" >&2
      fail=1
      continue
    fi

    expected=$(grep -m1 '^version = ' "$toml" | sed -E 's/^version = "([^"]+)".*/\1/')
    if [ -z "$expected" ]; then
      echo "::error file=${toml}::could not extract version" >&2
      fail=1
      continue
    fi

    if [ "$pinned" != "$expected" ]; then
      echo "::error file=${readme}::stale pin for ${crate}: README pins v${pinned} but ${toml} is ${expected}. Update the tag in ${readme}." >&2
      fail=1
      continue
    fi

    echo "✓ ${readme}: ${crate} pin v${pinned} matches Cargo.toml"
  done < <(grep -oE 'tag = "[a-z][a-z0-9-]*-v[0-9]+\.[0-9]+\.[0-9]+"' "$readme" \
             | sed -E 's/^tag = "(.*)"$/\1/' || true)
done

exit $fail
