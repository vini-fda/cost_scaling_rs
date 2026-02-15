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

# Build with debug symbols (required for symbol resolution)
echo "==> Building release binary with debug symbols..."
CARGO_PROFILE_RELEASE_STRIP=none CARGO_PROFILE_RELEASE_DEBUG=2 cargo build --release --quiet

# Generate dSYM (required on macOS for samply symbol resolution)
echo "==> Generating dSYM..."
dsymutil target/release/cost-scaling-rs 2>/dev/null || true

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

# Create a wrapper that runs multiple iterations for better sampling
WRAPPER=$(mktemp)
trap "rm -f $WRAPPER" EXIT
cat > "$WRAPPER" << EOF
#!/usr/bin/env bash
for i in \$(seq 1 $ITERATIONS); do
    $(pwd)/target/release/cost-scaling-rs $(pwd)/$PROBLEM > /dev/null
done
EOF
chmod +x "$WRAPPER"

echo "==> Profiling $ITERATIONS iterations of $(basename "$PROBLEM")..."
echo "    (Each run takes ~300ms, total ~$((ITERATIONS * 300 / 1000))s)"
echo ""

samply record "${SAMPLY_ARGS[@]}" "$WRAPPER"

if [[ " ${SAMPLY_ARGS[*]} " == *" --save-only "* ]]; then
    echo ""
    echo "==> Profile saved to profile.json"
    echo "    View at: https://profiler.firefox.com/ (click Load...)"
fi
