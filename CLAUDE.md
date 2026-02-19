# Project Overview

This project (cost_scaling_rs) is a Rust translation of the original CS2 min-cost maximum-flow cost scaling algorithm.
It aims to match the performance of the C implementation using 100% native Rust, with good documentation and usage of Rust idioms.

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
- `OPTIMIZATION.md` - Documentation on the optimization process.
  - New sections should be added as you learn

# Context and how to work on this repo

This project's goal is for the Rust implementation to match (or improve) the C's code performance,
with extremely well-documented and battle-tested code. This will involve:

- deep knowledge of graph algorithms
- deep knowledge of Rust patterns, such as typestate, strong typing, RAII and so on
- deep knowledge of unsafe Rust, and how to make safe wrappers around unsafe APIs
- benchmarking and comparing with the C code's performance in a measurable, objective way
- formulating hypotheses, testing, iterating on them then documenting the process

The current state is that:

- the performance does not match the C performance. 
- functions, types and variables in the Rust code are poorly named or poorly documented, especially methods in `McmfCs2`.
- some datatypes and data structures are not isolated into their own high-performance APIs.
  - this makes e.g. testing a "Bucket" implementation in isolation difficult

First, read README.md to get an overview. Then, read OPTIMIZATION.md to get the current state of optimization.

The transcribed Goldberg paper in `docs/main.typ` gives a thorough explanation of the reasoning behind concepts, decisions, heuristics and optimizations of the original code. It should be seen as a guide.
