#!/usr/bin/env bash
set -euo pipefail

crate="${1:?usage: check-readme-pins.sh <crate>}"
repo_url="https://github.com/BotResources/br-rust-common"
root_readme="README.md"
version=$(bash "$(dirname "$0")/crate-version.sh" "$crate")

if [ "$version" = "0.0.0" ]; then
  echo "✓ ${crate} 0.0.0 scaffold — no tag has shipped, no self-pin to check"
  exit 0
fi

if [ ! -f "$root_readme" ]; then
  echo "::error::${root_readme} is missing" >&2
  exit 1
fi

expected_tag="${crate}/v${version}"
expected_pin="git = \"${repo_url}\", package = \"${crate}\", tag = \"${expected_tag}\", version = \"${version}\""
short_pin="package = \"${crate}\", tag = \"${expected_tag}\", version = \"${version}\""

if ! grep -qF "${expected_pin}" "$root_readme"; then
  echo "::error file=${root_readme}::the ${crate} install pin must read { ${expected_pin} } (a tag-only git dep is a wildcard and cargo-deny denies it)" >&2
  exit 1
fi

fail=0
for readme in "$root_readme" "crates/${crate}/README.md"; do
  [ -f "$readme" ] || continue
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    if ! grep -qF "${short_pin}" <<<"$line"; then
      echo "::error file=${readme}::${crate} self-pin is stale: ${line}" >&2
      fail=1
    fi
  done < <(grep -F "package = \"${crate}\"" "$readme" | grep -F "tag = \"" || true)
done

if [ "$fail" -eq 0 ]; then
  echo "✓ ${crate} self-pins on ${expected_tag} / ${version}"
fi

exit $fail
