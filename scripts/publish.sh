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
#   2  rill-core-actor
#   3  rill-core-dsp
#   4  rill-core-wdf
#   5  rill-graph
#   6  rill-telemetry
#   7  rill-lofi
#   8  rill-patchbay
#   9  rill-oscillators
#  10  rill-digital-filters
#  11  rill-digital-effects
#  12  rill-router
#  13  rill-io
#  14  rill-analog-filters
#  15  rill-analog-effects
#  16  rill-osc
#  17  rill-sampler
#  18  rill-adrift

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

CRATES=(
    rill-core
    rill-core-actor
    rill-core-dsp
    rill-core-wdf
    rill-graph
    rill-telemetry
    rill-lofi
    rill-io
    rill-oscillators
    rill-digital-filters
    rill-digital-effects
    rill-router
    rill-patchbay
    rill-analog-filters
    rill-analog-effects
    rill-osc
    rill-sampler
    rill-adrift
)

DRY_RUN=false
RESUME=0
PUBLISH_COUNT=0
BURST_LIMIT=5
BURST_WAIT=600   # 10 minutes
INDEX_WAIT=30    # 30 seconds for index update

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
            echo "  Rate limited (429). Waiting 10 minutes before retry..."
            sleep "$BURST_WAIT"
            PUBLISH_COUNT=0  # reset burst counter after forced pause
            echo "  Retrying $crate..."
            cargo publish -p "$crate" 2>&1
            echo "  ✓ published $crate"
        else
            echo "$output"
            if echo "$output" | grep -q "^error"; then
                echo "  ✗ publish FAILED"
                exit 1
            fi
            echo "  ✓ published $crate"
        fi

        PUBLISH_COUNT=$((PUBLISH_COUNT + 1))
        if [ "$PUBLISH_COUNT" -ge "$BURST_LIMIT" ]; then
            echo "  Burst limit ($BURST_LIMIT) reached. Waiting ${BURST_WAIT}s before next publish..."
            sleep "$BURST_WAIT"
            PUBLISH_COUNT=0
        else
            echo "  Waiting ${INDEX_WAIT}s for crates.io index to update..."
            sleep "$INDEX_WAIT"
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
