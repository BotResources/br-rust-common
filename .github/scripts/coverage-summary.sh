#!/usr/bin/env bash
# coverage-summary.sh — produce a per-crate × {UT-only, UT+E2E} markdown
# table from two `cargo llvm-cov report --json` snapshots.
#
# Usage:
#   coverage-summary.sh <ut.json> <ut-e2e.json>
#
# Writes the markdown table to stdout. Intended to be appended to
# $GITHUB_STEP_SUMMARY by the `coverage` job in ci.yml.
#
# Why per-crate: this workspace publishes 5 independently-versioned crates
# pinned crate-by-crate by consumers. A single workspace-average figure
# hides outliers — `br-util-postgres` carries the risky SQL logic, while
# `br-core-kernel` is mostly newtypes. The Δ column shows how much of
# each crate is covered *only* by the e2e PG tests, so we can see at a
# glance what the e2e-postgres job is actually protecting.

set -euo pipefail
# C locale → printf "%.1f" uses '.' not ','. Without this, comma-locale
# runners (and macOS dev shells) would emit "82,6%" which breaks any
# downstream parsing that expects a numeric percent.
export LC_ALL=C

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <ut.json> <ut-e2e.json>" >&2
    exit 2
fi

ut_json="$1"
e2e_json="$2"

for f in "$ut_json" "$e2e_json"; do
    if [ ! -s "$f" ]; then
        echo "error: $f missing or empty" >&2
        exit 1
    fi
done

# Per-crate aggregation: emit one row "<crate>\t<lines_covered>\t<lines_total>"
# for every file under crates/<name>/, summed by crate.
#
# llvm-cov's JSON emits absolute paths in the runner workspace; we match
# the `/crates/<name>/` segment and bucket by `<name>`.
aggregate_per_crate() {
    # POSIX awk: no `match($1, re, arr)` capture-group support. Split on
    # `/` and locate the segment immediately after `crates`.
    jq -r '.data[0].files[] | [.filename, .summary.lines.covered, .summary.lines.count] | @tsv' "$1" \
        | awk -F'\t' '
            {
                n = split($1, parts, "/")
                for (i = 1; i <= n; i++) {
                    if (parts[i] == "crates" && i < n) {
                        crate = parts[i + 1]
                        covered[crate] += $2
                        total[crate]   += $3
                        break
                    }
                }
            }
            END {
                for (c in covered) printf "%s\t%d\t%d\n", c, covered[c], total[c]
            }
        '
}

ws_totals() {
    jq -r '.data[0].totals.lines | "\(.covered)\t\(.count)"' "$1"
}

# Single awk pass: read UT rows, then E2E rows (separated by sentinel),
# then emit the markdown table.
{
    aggregate_per_crate "$ut_json"
    echo "---SPLIT---"
    aggregate_per_crate "$e2e_json"
    echo "---SPLIT---"
    ws_totals "$ut_json"   # one line, no crate prefix
    echo "---SPLIT---"
    ws_totals "$e2e_json"
} | awk '
    BEGIN {
        section = 0
        FS = "\t"
    }
    /^---SPLIT---$/ { section++; next }
    section == 0 { ut_c[$1] = $2; ut_t[$1] = $3; crates[$1] = 1 }
    section == 1 { e2e_c[$1] = $2; e2e_t[$1] = $3; crates[$1] = 1 }
    section == 2 { ut_ws_c = $1; ut_ws_t = $2 }
    section == 3 { e2e_ws_c = $1; e2e_ws_t = $2 }
    END {
        print "## Coverage (line)"
        print ""
        print "| crate | UT only | UT + E2E | Δ E2E |"
        print "|---|---:|---:|---:|"

        # Sort crate names for stable output.
        n = 0
        for (c in crates) { keys[++n] = c }
        # Simple selection sort — n is small (~5).
        for (i = 1; i <= n; i++) {
            for (j = i + 1; j <= n; j++) {
                if (keys[j] < keys[i]) { tmp = keys[i]; keys[i] = keys[j]; keys[j] = tmp }
            }
        }

        for (i = 1; i <= n; i++) {
            c = keys[i]
            uc = ut_c[c]   + 0; utt  = ut_t[c]  + 0
            ec = e2e_c[c]  + 0; et   = e2e_t[c] + 0
            up = (utt > 0) ? (uc / utt) * 100 : 0
            ep = (et  > 0) ? (ec / et)  * 100 : 0
            delta = ep - up

            up_s = (utt > 0) ? sprintf("%.1f%%", up) : "—"
            ep_s = (et  > 0) ? sprintf("%.1f%%", ep) : "—"
            if (delta == 0 || (utt == 0 && et == 0)) {
                d_s = "—"
            } else {
                d_s = sprintf("%+.1fpp", delta)
            }
            printf "| `%s` | %s | %s | %s |\n", c, up_s, ep_s, d_s
        }

        # Workspace row from llvm-cov totals (sums across all files, not
        # only crates/* — but in this repo that is the same set).
        uc = ut_ws_c + 0; utt = ut_ws_t + 0
        ec = e2e_ws_c + 0; et = e2e_ws_t + 0
        up = (utt > 0) ? (uc / utt) * 100 : 0
        ep = (et  > 0) ? (ec / et)  * 100 : 0
        delta = ep - up
        up_s = sprintf("%.1f%%", up)
        ep_s = sprintf("%.1f%%", ep)
        d_s  = (delta == 0) ? "—" : sprintf("%+.1fpp", delta)
        printf "| **workspace** | **%s** | **%s** | **%s** |\n", up_s, ep_s, d_s
    }
'
