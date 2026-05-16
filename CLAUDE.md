# Project Overview

This project (cost_scaling_rs) is a Rust translation of the original CS2 min-cost maximum-flow cost scaling algorithm.
As of the `perf/raw-pointer-arenas-v2` branch, the Rust implementation **matches or slightly beats** the C reference on the GOTO benchmark (Rust 1.02–1.06× faster on n ≥ 5000, ties on smaller problems). Continued work should preserve that parity while improving safety, documentation, and Rust idioms.

# Commit rules
- Keep commit messages without any co-author
- The master branch is protected
- Changes must be made in new branches, then a PR must be opened to merge to master
- CI checks must pass before merging (`.github/workflows/rust.yml`)

# Build & Test Commands
- Build: `cargo build`
- Test: `cargo test`
- Run: `cargo run`
- Lint: `cargo clippy`
- Format: `cargo fmt`
- Look at the `justfile` to see project-specific commands

# Code Style
- Follow Rust 2024 edition conventions
- Never use external dependencies (except for dev-dependencies)
- Use `Result<T, E>` for error handling, avoid `.unwrap()` in production code
- Document public APIs with `///` doc comments

# Project Structure
- `cs2/` - Reference C implementation, with very few adjustments to make it compile under modern C standards.
- `src/lib.rs` - The entire min-cost max-flow algorithm is here.
- `src/goto.rs` - GOTO (Grid On TOrus) problem generator in DIMACS format.
- `src/parser.rs` - Parser for DIMACS minimum-cost flow (`.min`) files.
- `src/main.rs` - Binary entry point.
- `tests/` - Integration tests, comparing the Rust implementation to the C implementation.
  - `testdata/` - Sample test data.
- `benches/` - Criterion benchmarks.
- `examples/gen_goto.rs` - GOTO problem generator CLI.
- `docs/` - Documentation.

# Context and how to work on this repo

This project's goal is to keep the Rust implementation at parity with (or ahead of) the C reference, with extremely well-documented and battle-tested code. Working on it well involves:

- deep knowledge of graph algorithms
- deep knowledge of Rust patterns, such as typestate, strong typing, RAII and so on
- deep knowledge of unsafe Rust, and how to make safe wrappers around unsafe APIs
- benchmarking and comparing with the C code's performance in a measurable, objective way
- formulating hypotheses, testing, iterating on them then documenting the process

## Current state

- **Performance matches or beats C** on the GOTO benchmark (see numbers above). The win came from converting structural links (`Arc::head`, `Arc::sister`, `Node::first`/`current`/`suspended`/`q_next`/`b_next`/`b_prev`, `Bucket::p_first`, and the `McmfCs2` sentinel/queue head fields) from `usize` indexes to raw pointers, matching the C struct layout 1:1 so that `a->head->price` is a single load with offset instead of `base + idx*sizeof + offset`.
- The hot methods (`discharge`, `refine`, `price_update`, `price_refine`, `compute_prices`, `price_in`, `price_out`, `relabel`, `up_node_scan`, etc.) are inside crate-level `unsafe` blocks that dereference cached base pointers (`nodes_base`, `arcs_base`, `buckets_base`).
- Safety invariants are documented in `// SAFETY:` comments; `tests/miri_soundness.rs` runs under miri (Stacked Borrows and Tree Borrows — both clean in CI) and exercises every `unsafe` block.
- The library forbids `panic!`/`unwrap`/`expect`/`unreachable!`/`todo!`/`unimplemented!`/`panic_in_result_fn` in non-test code via `#![cfg_attr(not(test), deny(...))]`. Build-time and solver-time errors are surfaced through `Result<_, Cs2Error>`; the DIMACS loaders return `DimacsLoadError` (wrapping either `ParseError` or `Cs2Error`).
- Some functions, types, and variables in the Rust code are still poorly named or poorly documented, especially methods in `McmfCs2`. Naming/doc improvements are welcome but **must not regress the bench-compare numbers** — re-run `just bench-compare` (or `cargo run --release --example bench_compare`) after non-trivial changes.
- Some data structures (e.g. the bucket array) are not isolated into their own high-performance APIs, which makes testing them in isolation harder.

## Performance regression discipline

Before merging anything that touches the hot path (`src/lib.rs` solver methods, struct layouts, or codegen-relevant attributes):

1. Run `just bench-compare` against the C reference at n = 2000/5000/10000.
2. If a change costs more than ~2% on any size, justify it in the PR or roll it back.
3. If a change adds new `unsafe`, also run `just miri` (Stacked + Tree Borrows).

## Reading order for newcomers

1. `README.md` — public API and how to run the solver.
2. `docs/main.typ` — transcribed Goldberg paper, explains the heuristics and optimizations.
3. `src/lib.rs` — the algorithm itself; `cs2()` is the entry point of the solve loop, `refine`/`price_refine` are the main inner phases.
4. `cs2/cs2.c` — the C reference, useful when puzzling out an opaque heuristic.
