#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: previous-tag.sh <crate>}"

version=$(git tag --list "${crate}/v*" | sed -E "s#^${crate}/v##" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n1 || true)
if [ -n "$version" ]; then
  echo "${crate}/v${version}"
fi
