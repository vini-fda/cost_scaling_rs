#!/usr/bin/env -S just --justfile

set shell := ["bash", "-euo", "pipefail", "-c"]

# ---------------------------------------------------------------------------
# Core workflows
# ---------------------------------------------------------------------------
fmt:
  cargo fmt --all

fmt-check:
  cargo fmt --all -- --check

clippy:
  cargo clippy --workspace --all-targets -- -D warnings

test +args='':
  cargo test --workspace {{args}}

build profile='release':
  cargo build --workspace --profile {{profile}}

run +args='testdata/sample.inp':
  cargo run -- {{args}}

clean:
  cargo clean

ci: fmt-check clippy test

# Run the miri soundness suite under both aliasing models.
# Requires the `miri` component on a nightly toolchain:
#   rustup +nightly component add miri
miri:
  MIRIFLAGS="-Zmiri-disable-isolation" \
    cargo +nightly miri test --test miri_soundness
  MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-tree-borrows" \
    cargo +nightly miri test --test miri_soundness

# ---------------------------------------------------------------------------
# Benchmarks & performance analysis
# ---------------------------------------------------------------------------
bench:
  cargo bench

bench-compare +args='':
  cargo run --release --example bench_compare -- {{args}}

benchdata dir='target/benchdata':
  cargo run --release --example gen_goto -- {{dir}}

profile size='10000' iterations='20':
  cargo run --example profile -- --size {{size}} --iterations {{iterations}}

profile-save size='10000' iterations='20':
  cargo run --example profile -- --size {{size}} --iterations {{iterations}} --save-only

profile-extract src='profile.json.gz' out='profile.json':
  gzip -dc {{src}} > {{out}}
  echo "Profile JSON ready at {{out}}. Load it at https://profiler.firefox.com/."
