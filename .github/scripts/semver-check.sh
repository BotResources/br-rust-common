#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: semver-check.sh <crate>}"
lib="${crate//-/_}"
scripts="$(cd "$(dirname "$0")" && pwd)"

baseline_tag=$(bash "$scripts/previous-tag.sh" "$crate")
if [ -z "$baseline_tag" ]; then
  echo "::notice::no prior ${crate}/v* tag, skipping semver-checks (first release)"
  exit 0
fi

base_wt="$(mktemp -d)/baseline"
git worktree add --detach "$base_wt" "$baseline_tag"
trap 'git worktree remove --force "$base_wt" 2>/dev/null || true; git worktree prune' EXIT

rustdoc_json() {
  local dir="$1" out="$2"
  ( cd "$dir" && RUSTC_BOOTSTRAP=1 \
    RUSTDOCFLAGS="-Z unstable-options --output-format json --cap-lints allow" \
    cargo doc -p "$crate" --lib --no-deps --all-features --locked \
    --target-dir "$out" )
}

rustdoc_json "." "$PWD/target/semver-current-${crate}"
rustdoc_json "$base_wt" "$base_wt/target/semver-baseline"
cargo semver-checks -p "$crate" \
  --baseline-rustdoc "$base_wt/target/semver-baseline/doc/${lib}.json" \
  --current-rustdoc "$PWD/target/semver-current-${crate}/doc/${lib}.json"
