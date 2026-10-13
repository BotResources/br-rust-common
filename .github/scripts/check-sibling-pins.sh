#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <crate> <cargo-metadata.json>" >&2
    exit 2
fi

CRATE="$1"
METADATA="$2"
source "$(dirname "${BASH_SOURCE[0]}")/tracked-libraries.sh"

python3 - "$METADATA" "$CRATE" "${TRACKED_LIBRARIES:-}" <<'PY'
import json, re, sys

metadata_file, crate, tracked = sys.argv[1], sys.argv[2], sys.argv[3].split()
meta = json.load(open(metadata_file))
packages = {p["id"]: p for p in meta["packages"]}
members = set(meta["workspace_members"])
root = next(p for p in meta["packages"] if p["name"] == crate and p["id"] in members)
node = next(n for n in meta["resolve"]["nodes"] if n["id"] == root["id"])

pinned = set()
for declared in root["dependencies"]:
    if re.search(r"[?&]tag=" + re.escape(declared["name"]) + r"/v[0-9]", declared.get("source") or ""):
        pinned.add(declared["name"])

bare = sorted(
    packages[dep["pkg"]]["name"]
    for dep in node["deps"]
    if dep["pkg"] in members
    and packages[dep["pkg"]]["name"] in tracked
    and packages[dep["pkg"]]["name"] != crate
    and any(kind["kind"] != "dev" for kind in dep["dep_kinds"])
    and packages[dep["pkg"]]["name"] not in pinned
)
for sibling in bare:
    print(
        "::error::%s depends on its tracked sibling %s without a tag pin: a consumer would resolve a second copy of %s. "
        "Release %s first, then depend on it as { git = \"https://github.com/BotResources/br-rust-common\", tag = \"%s/v<version>\", version = \"<version>\" } "
        "with the root [patch] to its local path." % (crate, sibling, sibling, sibling, sibling),
        file=sys.stderr,
    )
sys.exit(1 if bare else 0)
PY
