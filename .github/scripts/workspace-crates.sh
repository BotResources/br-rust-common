#!/usr/bin/env bash
set -euo pipefail

for manifest in crates/*/Cargo.toml; do
  [ -f "$manifest" ] || continue
  basename "$(dirname "$manifest")"
done
