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

The hot line in `relabel`'s inner loop illustrates the core difference between the two implementations.

**C** (`cs2/cs2.c:619`):
```c
dp = ((a -> head) -> price) - (a -> cost);
```

`a` is a pointer to an `arc` struct. `a->head` is a pointer to a `node` struct. Each `->` is a single load at a fixed offset from the pointer — no arithmetic beyond the offset. Total: 3 loads.

**Rust** (`src/lib.rs:1091–1092`):
```rust
let head = self.arcs[a].head;
let dp = self.nodes[head].price - self.arcs[a].cost;
```

`a` and `head` are integer indices. Each indexing operation requires a base pointer + index × stride calculation. The compiler can strength-reduce the stride multiply for `self.arcs[a]` (since `a` increments by 1 each iteration), but the access through `self.nodes[head]` has a **data-dependent address**: the index loaded from `arc.head` must be multiplied by `sizeof(Node)` before the node can be accessed. In C, `a->head` is already a pointer — no multiplication needed.

This dependent address calculation is the primary source of the ~10% gap. It is inherent to the index-based design (chosen for memory safety and `#![forbid(unsafe_code)]`) and is not something bounds-check elimination can help with, as confirmed by the unchecked indexing experiment above.

## Assembly analysis of the hot loop (confirmed)

Inspecting the generated AArch64 assembly for `relabel`'s inner loop (Apple Silicon, `--release` with LTO) confirms the hypothesis above. The critical path per iteration is:

```asm
ldur  x10, [x3, #-16]        ; load arcs[a].res_capacity
; ... branch if <= 0 ...
ldr   x10, [x3]              ; load arcs[a].head (an index)
madd  x10, x10, x5, x12     ; x10 = head * 80 + nodes_base  ← THE MULTIPLY
ldr   x10, [x10, #32]        ; load nodes[head].price
ldur  x6, [x3, #-8]          ; load arcs[a].cost
sub   x10, x10, x6           ; dp = price - cost
```

For comparison, the C version's inner loop (compiled with `gcc -O3`, AArch64):

```asm
ldr   x16, [x15]            ; load arcs[a].res_capacity
; ... branch if < 1 ...
ldp   x17, x16, [x15, #8]  ; load cost AND head pointer in ONE instruction
ldr   x16, [x16, #32]       ; load head->price (direct pointer chase)
sub   x16, x16, x17         ; dp = price - cost
```

Two key differences:

1. **No multiply.** C's `a->head` is already a `node*`. The load at `[x16, #32]` goes straight to `price` with a fixed offset — no address arithmetic beyond the offset. In Rust, the `madd` instruction (multiply-add) computes `head_index * sizeof(Node) + base_pointer`, adding latency to the critical dependency chain: load index → multiply → load price (3 dependent ops) vs C's load pointer → load price (2 dependent ops).

2. **Load pair (`ldp`).** GCC loads both `cost` and `head` in a single `ldp` instruction from adjacent struct fields. LLVM does not emit `ldp` for the Rust version because the fields are accessed at different points in the loop body and the struct layout differs (indices vs pointers).

`sizeof(Node) = 80` is not a power of 2, so the compiler cannot replace the multiply with a shift. The sequential arc access (`self.arcs[a]` where `a` increments by 1) is strength-reduced by LLVM into pointer increments (`add x3, x3, #32`), so the arc stride multiply is already eliminated. Only the data-dependent node access pays the cost.

## Shrinking Node to 64 bytes (rejected)

**Hypothesis:** If `sizeof(Node)` were 64 (a power of 2), the `madd` could become `add + lsl #6`, removing the multiply from the critical path.

**Attempt:** Changed `Price` and `Excess` from `i64` to `i32`, and `rank` from `i64` to `i32`. This brought `sizeof(Node)` down to 64 bytes.

**Result:** Tests pass on small problems (up to 2000 nodes), but the binary **hangs on problems with ≥5000 nodes**. The cost-scaling algorithm multiplies costs by `n` during initialization and accumulates prices as multiples of `epsilon = n * max_cost`. For 5000+ nodes these intermediate values exceed `i32::MAX` (~2.1 billion), and in release mode i32 overflow wraps silently, corrupting algorithm invariants and causing infinite loops in `price_update`.

**Conclusion:** `Price` and `Excess` must remain `i64` — the original C code uses `long long` for exactly this reason. Achieving 64-byte nodes would require removing a field (e.g. `b_prev`, converting doubly-linked bucket lists to singly-linked), which risks degrading `remove_from_bucket` from O(1) to O(bucket_size). Not pursued.
