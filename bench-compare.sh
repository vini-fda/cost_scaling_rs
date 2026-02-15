#!/usr/bin/env bash
#
# bench-compare.sh — End-to-end timing comparison between the Rust and C
# implementations of the CS2 min-cost max-flow solver.
#
# Dependencies:
#   - Rust toolchain (cargo, rustc)
#   - C compiler (gcc or cc)
#   - make
#   - hyperfine (https://github.com/sharkdp/hyperfine)
#       Install: cargo install hyperfine  OR  brew install hyperfine
#
# Usage:
#   ./bench-compare.sh              # run all benchmarks
#   ./bench-compare.sh --warmup 5   # pass extra args to hyperfine
#
# The script will:
#   1. Build the Rust binary in release mode (LTO + single codegen unit)
#   2. Build the C binary with -O3 -DNDEBUG
#   3. Generate GOTO test problems of increasing size
#   4. Run hyperfine comparing both binaries on each problem

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RUST_BIN="target/release/cost-scaling-rs"
C_BIN="cs2/cs2"
DATA_DIR="target/benchdata"
HYPERFINE_ARGS=("--warmup" "3" "--min-runs" "20" "$@")

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------

check_dep() {
    if ! command -v "$1" &>/dev/null; then
        echo "Error: '$1' is required but not found." >&2
        echo "  $2" >&2
        exit 1
    fi
}

check_dep cargo   "Install from https://rustup.rs"
check_dep make    "Install build-essential (Linux) or Xcode CLI tools (macOS)"
check_dep hyperfine "Install: cargo install hyperfine  OR  brew install hyperfine"

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

echo "==> Building Rust binary (release, LTO)..."
cargo build --release --quiet

echo "==> Building C binary (gcc -O3)..."
make -C cs2 clean --quiet 2>/dev/null || true
make -C cs2 cs2.exe \
    CCOMP=gcc \
    "CFLAGS=-O3 -DNDEBUG -DPRINT_ANS -DCOMP_DUALS" \
    --quiet 2>/dev/null

if [[ ! -x "$RUST_BIN" ]]; then
    echo "Error: Rust binary not found at $RUST_BIN" >&2
    exit 1
fi
if [[ ! -x "$C_BIN" ]]; then
    echo "Error: C binary not found at $C_BIN" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Generate test data
# ---------------------------------------------------------------------------

echo "==> Generating GOTO test problems..."
cargo run --release --quiet --example gen_goto -- "$DATA_DIR"

# ---------------------------------------------------------------------------
# Run benchmarks
# ---------------------------------------------------------------------------

echo ""
echo "======================================================================"
echo "  Rust ($RUST_BIN) vs C ($C_BIN)"
echo "  hyperfine args: ${HYPERFINE_ARGS[*]}"
echo "======================================================================"
echo ""

for problem in "$DATA_DIR"/goto_*.min; do
    basename="$(basename "$problem" .min)"
    echo "--- $basename ---"
    hyperfine "${HYPERFINE_ARGS[@]}" \
        --command-name "Rust" "$RUST_BIN $problem > /dev/null" \
        --command-name "C"    "$C_BIN < $problem > /dev/null"
    echo ""
done
