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

## BucketArray extraction: Node 80 → 64 bytes (committed)

**Hypothesis:** The `b_next`, `b_prev`, and `rank` fields in `Node` (24 bytes) are only used in `price_update` and `price_refine`, not in the hottest loops (`relabel`, `discharge`). Moving them to a separate `BucketArray` struct (parallel arrays) shrinks `Node` from 80 to 64 bytes — a power of 2 — without touching any numeric types.

**Change:** Introduced `BucketArray { p_first, b_next, b_prev, rank }` as a standalone struct. The bucket-list head `p_first` was already a `Vec` on `McmfCs2`; the per-node fields moved out of `Node`. `sizeof(Node)` went from 80 to 64 bytes.

**Result:**

| Problem | Rust (before) | Rust (after) | C | Ratio (after) |
|---------|--------------|--------------|---|---------------|
| 500n    | 6.8ms        | ~4.6ms       | ~4.3ms | 1.07x |
| 2000n   | 40.5ms       | ~33.6ms      | ~30.4ms | 1.11x |
| 5000n   | 116.3ms      | ~103.8ms     | ~93.6ms | 1.11x |
| 10000n  | 314.3ms      | ~281.1ms     | ~250.5ms | 1.12x |

Absolute improvement ~10% across all sizes. The gap relative to C remained at ~11–12%.

**Why:** Denser nodes improve cache utilization: more nodes fit per cache line in the `price_update` sweep and the `discharge` / `relabel` node accesses. The power-of-2 stride was a secondary gain (confirmed by assembly update below).

## target-cpu=native (committed)

**Hypothesis:** The bench-compare script passes `-march=native -flto` to GCC but Rust builds had no equivalent. Adding `-C target-cpu=native` via `.cargo/config.toml` levels the playing field and may enable LLVM to use Apple Silicon-specific instructions.

**Change:** Added `.cargo/config.toml`:
```toml
[build]
rustflags = ["-C", "target-cpu=native"]
```

**Result:** No measurable improvement. Ratios at 2000n–10000n unchanged (~1.10–1.12x). Apple Silicon's AArch64 ISA has few optional extensions that affect integer code; the default `aarch64-apple-darwin` target already enables the relevant ones. The bottleneck is memory latency, not instruction selection.

## Updated assembly analysis (Node = 64 bytes, target-cpu=native)

Regenerated AArch64 assembly for `relabel`'s inner loops after the BucketArray extraction:

**Rust** (both scanning loops in `relabel`, representative loop body):
```asm
ldur  x9, [x2, #-16]       ; arcs[a].res_capacity  (x2 = &arc.head)
cmp   x9, #0
b.le  <next>               ; skip non-positive residual
ldr   x9, [x2]             ; arcs[a].head  (index)
add   x9, x13, x9, lsl #6  ; nodes_base + head * 64  ← lsl #6, NOT madd
ldr   x9, [x9, #32]        ; nodes[head].price
ldur  x4, [x2, #-8]        ; arcs[a].cost
sub   x9, x9, x4           ; dp = price - cost
```

The `madd` (multiply-add) from the 80-byte era is gone. The stride multiplication is now `lsl #6` folded into the addressing of a standard `add` — a single 1-cycle instruction on Apple Silicon vs the 3-cycle `madd`.

**Remaining structural difference vs C:**

C's two-step chain:
```
load head-pointer  →  load head->price     (2 dependent loads)
```

Rust's three-step chain:
```
load head-index  →  add+lsl #6 (1 cy)  →  load nodes[head].price     (2 dependent loads + 1 arithmetic)
```

The extra `add+lsl` step sits on the critical dependency path. With an L1 hit (~4 cy), the arithmetic adds ~25% extra latency to that chain segment. With an L2 miss the arithmetic is hidden in the miss latency and the difference shrinks — which explains why the gap narrows at larger problem sizes (more cache pressure, longer miss latencies dominate).

**`ldp` not emitted:** `cost` (at `[x2, #-8]`) and `head` (at `[x2]`) are adjacent in memory, so a `ldp x4, x9, [x2, #-8]` would load both in one instruction (matching GCC's behavior on the C struct). LLVM does not emit it because the two values are consumed at different points in the loop body and register pressure differs. This is a missed optimization in LLVM's AArch64 backend for this pattern.

## u32 newtype indices: Node 64 → 32 bytes (committed)

**Hypothesis:** Replacing `NodeIndex = usize` and `ArcIndex = usize` type aliases
with `NodeIdx(u32)` and `ArcIdx(u32)` newtype wrappers shrinks struct sizes:

- `Node`: 64 bytes → 32 bytes (`first`, `current`, `suspended`, `q_next` each
  drop from 8 bytes to 4 bytes; `dfs_parent` and `inp` moved to parallel arrays
  `McmfCs2::dfs_parent` and `McmfCs2::inp` to keep the hot-path struct lean)
- `Arc`: 32 bytes → 24 bytes (`head` and `sister` drop from 8 bytes to 4 bytes)
- `BucketArray` per-node arrays (`b_next`, `b_prev`): element size halved

The stride multiply for `nodes[head]` access was `lsl #6` (64-byte stride) and
becomes `lsl #5` (32-byte stride), saving 1 cycle on the 3-step critical path.
Twice as many nodes fit per L1/L2 cache line.

**Change:** Replaced type aliases with newtype wrappers. Added `::NONE`, `.idx()`,
and arithmetic impls (`Add<u32>`, `Sub<u32>`, `AddAssign<u32>`, `SubAssign<u32>`)
to `ArcIdx`. All call sites use `.idx()` for array indexing and `Foo(x as u32)`
for construction. All 53 tests pass.

**Result:**

| Problem | Rust (before, 64-byte Node) | Rust (after, 32-byte Node) | C | Ratio (after) |
|---------|----------------------------|---------------------------|---|---------------|
| 500n    | ~4.6ms                     | 4.5ms                     | 4.3ms | 1.05x |
| 2000n   | ~33.6ms                    | 32.8ms                    | 30.0ms | 1.10x |
| 5000n   | ~103.8ms                   | 102.1ms                   | 92.9ms | 1.10x |
| 10000n  | ~281.1ms                   | 273.4ms                   | 247.4ms | 1.11x |

~2–3% improvement across all problem sizes. The gap with C narrowed from ~11–12%
to ~10–11%.

**Why modest gains:** The `lsl #5` vs `lsl #6` saves 1 cycle per node access on
the hot path, but at large problem sizes most accesses miss L2 and the arithmetic
latency is hidden behind the ~50+ cy miss penalty. The benefit is most visible at
500n (fits in L1/L2) where the ratio improved most (1.07x → 1.05x).

**Assembly update** (32-byte Node, AArch64):
```asm
ldr   x9, [x2]             ; arcs[a].head  (index, u32)
add   x9, x13, x9, lsl #5  ; nodes_base + head * 32  ← lsl #5 (was lsl #6)
ldr   x9, [x9, #8]         ; nodes[head].price  (offset 8, was 32)
```

The price field is now at offset 8 instead of 32 (since Node is 32 bytes and
`excess` + `price` are still the first two fields). The stride multiply is
`lsl #5` vs the previous `lsl #6`.
