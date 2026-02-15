# Optimization notes

Performance experiments and findings from profiling the Rust implementation against the C reference.

## Baseline (before optimization)

Benchmarked with `bench-compare.sh` on GOTO problems (macOS, Apple Silicon):

| Problem | Rust | C | Ratio |
|---------|------|---|-------|
| 500n | 12.8ms (sys 6.9ms) | 6.8ms (sys 1.0ms) | 1.88x slower |
| 2000n | 61.2ms (sys 22.1ms) | 36.5ms (sys 1.0ms) | 1.70x slower |
| 5000n | 171.8ms (sys 54.1ms) | 105.2ms (sys 1.7ms) | 1.63x slower |
| 10000n | 419.8ms (sys 110.5ms) | 271.7ms (sys 2.5ms) | 1.54x slower |

The massive system-time overhead pointed to I/O, not computation.

## Buffered stdout (committed)

**Problem:** `println!` locks/unlocks stdout and flushes on every call. With 60k output lines, this caused ~60k small write syscalls.

**Fix:** Wrap stdout in `BufWriter` (`std::io::BufWriter`).

**Result:** System time dropped from ~110ms to ~4ms on the 10k problem. Overall speedup of ~25%.

| Problem | Rust (after) | C | Ratio |
|---------|-------------|---|-------|
| 500n | 6.8ms | 6.7ms | 1.01x |
| 2000n | 40.5ms | 36.5ms | 1.11x |
| 5000n | 116.3ms | 105.1ms | 1.11x |
| 10000n | 314.3ms | 279.5ms | 1.12x |

## Heap profiling with dhat (investigated)

Used `dhat` (v0.3) to profile heap allocations. Found only **33 total allocations** (~17.6MB), all during initialization:

- `parser::parse` — 5.2MB (Vec growing during parsing)
- `allocate_arrays` — ~7.3MB (nodes, arcs, cap, arc_tail, arc_first)
- `cs2_initialize` — 960KB (bucket array)
- File I/O — 3.2MB (reading input file into String)

**Conclusion:** No allocations in the hot loop. Heap allocation is not a bottleneck.

## CPU profiling with samply (investigated)

Sampled the solver on the 10k-node problem. Results (315 samples):

| Self time | Function |
|-----------|----------|
| 31.1% | `price_update` (bucket scanning loop) |
| 35.3% | `relabel` (arc scanning loops) |
| 21.3% | `main` (stdout I/O) |
| 1.3% | `price_in` |

The hot path is `price_update` + `relabel`, which together account for ~66% of CPU time.

## Unchecked indexing in relabel (rejected)

**Hypothesis:** Bounds checking on `self.arcs[a]` and `self.nodes[head]` in `relabel`'s inner loops (4 checked accesses per iteration) accounts for the remaining ~11% gap with C.

**Experiment:** Introduced `get_unchecked()` for arc and node accesses in `relabel`'s two scanning loops, gated behind an `unsafe-indexing` feature flag.

**Result:** Zero measurable difference.

```
Rust (safe):             309.2ms ± 6.3ms
Rust (unsafe-indexing):  309.1ms ± 5.1ms
C reference:             282.7ms ± 7.4ms
```

**Why no effect:** With `lto = "fat"` and `codegen-units = 1`, LLVM already eliminates bounds checks in these tight loops. The optimizer can prove the indices are in range from the loop bounds and the data structure invariants.

**Decision:** Reverted. The `#![forbid(unsafe_code)]` guarantee is more valuable than a 0% speedup.

## Remaining gap (~10%)

The ~10% CPU time gap between Rust and C likely comes from:

- **Index vs pointer indirection:** C traverses arcs/nodes via direct pointer dereference (`arc->head`), while Rust uses index-based access (`self.nodes[self.arcs[a].head]`), adding a base-pointer dependency.
- **Struct layout:** Both use equivalent field sizes (8 bytes each), but the compiler may pad or order fields differently.
- **Code generation differences:** LLVM may generate slightly different instruction sequences for Rust's iterator-based loops vs C's pointer arithmetic.

These are inherent to the index-based design (chosen for safety and simplicity over the C pointer-based approach) and are unlikely to be worth optimizing further.
