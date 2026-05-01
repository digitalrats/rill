#!/usr/bin/env bash
#
# publish.sh — publish all rill crates to crates.io in dependency order.
#
# Usage:
#   ./scripts/publish.sh            # publish all crates
#   ./scripts/publish.sh --check    # dry-run only (no actual publish)
#   ./scripts/publish.sh --resume N # start from crate N (1-indexed)
#
# Dependency order (leaf → root):
#   1  rill-core
#   2  rill-core-dsp
#   3  rill-core-wdf
#   4  rill-graph
#   5  rill-telemetry
#   6  rill-lofi
#   7  rill-patchbay
#   8  rill-oscillators
#   9  rill-digital-filters
#  10  rill-digital-effects
#  11  rill-router
#  12  rill-io
#  13  rill-analog-filters
#  14  rill-analog-effects
#  15  rill-osc
#  16  rill-adrift

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CRATES=(
    rill-core
    rill-core-dsp
    rill-core-wdf
    rill-graph
    rill-telemetry
    rill-lofi
    rill-patchbay
    rill-oscillators
    rill-digital-filters
    rill-digital-effects
    rill-router
    rill-io
    rill-analog-filters
    rill-analog-effects
    rill-osc
    rill-adrift
)

DRY_RUN=false
RESUME=0

while [ $# -gt 0 ]; do
    case "$1" in
        --check) DRY_RUN=true; shift ;;
        --resume) RESUME="${2:?Missing resume number}"; shift 2 ;;
        *) shift ;;
    esac
done

if [ "$RESUME" = "0" ] && [ "$DRY_RUN" = false ]; then
    echo "Usage: $0 [--check] [--resume N]"
    echo ""
    echo "  --check       dry-run (validate packages without publishing)"
    echo "  --resume N    start from crate N (1-indexed, see list below)"
    echo ""
    echo "Crates:"
    for i in "${!CRATES[@]}"; do echo "  $((i + 1))) ${CRATES[$i]}"; done
    exit 1
fi

if [ "$(git status --porcelain | wc -l)" -gt 0 ]; then
    echo "ERROR: working tree has uncommitted changes."
    echo "Commit or stash them first, then re-run."
    exit 1
fi

echo "Publishing ${#CRATES[@]} crates to crates.io"
echo "Dry run: $DRY_RUN"
echo ""

for i in "${!CRATES[@]}"; do
    idx=$((i + 1))
    crate="${CRATES[$i]}"

    if [ "$idx" -lt "$RESUME" ]; then
        echo "[$idx/${#CRATES[@]}] SKIP $crate (resume after $RESUME)"
        continue
    fi

    echo "============================================"
    echo "[$idx/${#CRATES[@]}] Publishing $crate ..."
    echo "============================================"

    if [ "$DRY_RUN" = true ]; then
        # Leaf crates (no internal deps) — full package verification
        if [ "$crate" = "rill-core" ] || [ "$crate" = "rill-osc" ]; then
            if cargo publish -p "$crate" --dry-run --allow-dirty 2>&1; then
                echo "  ✓ $crate publish dry-run passed"
            else
                echo "  ✗ $crate dry-run FAILED"
                exit 1
            fi
        # Dependent crates — validate manifest + compilation only
        else
            if cargo check -p "$crate" 2>&1; then
                echo "  ✓ $crate compiles (full publish check requires prior publish)"
            else
                echo "  ✗ $crate compilation FAILED"
                exit 1
            fi
        fi
    else
        echo "  Publishing $crate..."
        output=$(cargo publish -p "$crate" 2>&1) || true
        if echo "$output" | grep -q "429 Too Many Requests"; then
            retry_after=$(echo "$output" | grep -oP 'after \K.*?(?= GMT)')
            retry_after="${retry_after} GMT"
            retry_ts=$(date -d "$retry_after" +%s 2>/dev/null || echo "")
            if [ -n "$retry_ts" ]; then
                now=$(date +%s)
                wait_sec=$((retry_ts - now + 5))
                [ "$wait_sec" -lt 0 ] && wait_sec=0
                echo "  Rate limited. Waiting ${wait_sec}s until $retry_after..."
                sleep "$wait_sec"
                cargo publish -p "$crate" 2>&1
                echo "  ✓ published $crate"
            else
                echo "$output"
                echo "  ✗ rate limited, but could not parse retry time."
                exit 1
            fi
        else
            echo "$output"
            if echo "$output" | grep -q "^error"; then
                echo "  ✗ publish FAILED"
                exit 1
            fi
            echo "  ✓ published $crate"
        fi
        if [ "$idx" -gt 5 ]; then
            echo "  Rate limit cooldown: waiting 600s before next publish..."
            sleep 600
        else
            echo "  Waiting 30s for crates.io index to update..."
            sleep 30
        fi
    fi

    echo ""
done

echo "============================================"
if [ "$DRY_RUN" = true ]; then
    echo "All dry-runs passed. Ready to publish."
    echo "Run without --check to publish for real."
else
    echo "All crates published successfully!"
fi
echo "============================================"
