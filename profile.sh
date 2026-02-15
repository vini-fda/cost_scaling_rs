#!/usr/bin/env bash
set -euo pipefail

# CPU profiling with samply (opens Firefox Profiler in browser)
#
# Prerequisites:
#   cargo install samply
#
# Usage:
#   ./profile.sh              # profile 10k-node problem (default)
#   ./profile.sh --save-only  # save profile.json without opening browser
#   ./profile.sh --iterations 50  # run 50 iterations for better sampling

ITERATIONS=20
SAMPLY_ARGS=()
PROBLEM_SIZE="10000"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --save-only)
            SAMPLY_ARGS+=(--save-only -o profile.json)
            shift
            ;;
        --iterations)
            ITERATIONS="$2"
            shift 2
            ;;
        --size)
            PROBLEM_SIZE="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--save-only] [--iterations N] [--size NODES]"
            exit 1
            ;;
    esac
done

# Check dependencies
if ! command -v samply &>/dev/null; then
    echo "Error: samply not found. Install with: cargo install samply"
    exit 1
fi

# Build with debug symbols using the 'profiling' profile (inherits release, but unstripped)
echo "==> Building with profiling profile..."
cargo build --profile profiling --quiet

# Generate test data if missing
DATA_DIR="target/benchdata"
if [[ ! -d "$DATA_DIR" ]] || [[ -z "$(ls "$DATA_DIR"/*.min 2>/dev/null)" ]]; then
    echo "==> Generating test problems..."
    mkdir -p "$DATA_DIR"
    cargo run --release --quiet --example gen_goto -- "$DATA_DIR"
fi

# Find the problem file matching requested size
PROBLEM=$(ls "$DATA_DIR"/goto_${PROBLEM_SIZE}n_*.min 2>/dev/null | head -1 || true)
if [[ -z "$PROBLEM" ]]; then
    echo "Error: No problem file found for size ${PROBLEM_SIZE}."
    echo "Available:"
    ls "$DATA_DIR"/*.min 2>/dev/null | sed 's/^/  /'
    exit 1
fi

echo "==> Profiling $ITERATIONS iterations of $(basename "$PROBLEM")..."
echo "    (Each run takes ~300ms, total ~$((ITERATIONS * 300 / 1000))s)"
echo ""

samply record --iteration-count "$ITERATIONS" \
    ${SAMPLY_ARGS[@]+"${SAMPLY_ARGS[@]}"} \
    target/profiling/cost-scaling-rs "$PROBLEM"

if [[ ${#SAMPLY_ARGS[@]} -gt 0 && " ${SAMPLY_ARGS[*]} " == *" --save-only "* ]]; then
    echo ""
    echo "==> Profile saved to profile.json"
    echo "    View at: https://profiler.firefox.com/ (click Load...)"
fi
