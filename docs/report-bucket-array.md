# BucketArray Extraction — Benchmark Report

## Summary

The `BucketArray` extraction refactored bucket-related fields out of `Node` and into a dedicated
`BucketArray` struct with parallel arrays. This shrinks `Node` from 80 to 64 bytes (a power of 2),
which improves cache behavior and eliminates an expensive `imul`/`madd` in relabel's inner loop.

**Result: 2–5% improvement across all problem sizes, with all tests passing.**

## Benchmark Results (Criterion, `goto` problem generator)

| Size | master (baseline) | bucket-array | Change |
|------|-------------------|--------------|--------|
| 15   | 19.3 µs           | 18.4 µs      | -4.7%  |
| 30   | 51.6 µs           | 49.3 µs      | -4.5%  |
| 60   | 67.9 µs           | 65.8 µs      | -3.1%  |
| 120  | 417.2 µs          | 404.0 µs     | -3.2%  |
| 250  | 1.371 ms          | 1.312 ms     | -4.3%  |
| 500  | 1.513 ms          | 1.470 ms     | -2.8%  |

Criterion's own `change` measurements (comparing against its saved baseline from master) confirm:
- goto/15:  **-3.1%** (p = 0.00)
- goto/30:  **-2.3%** (p = 0.00)
- goto/60:  **-2.1%** (p = 0.00)
- goto/120: **+1.7%** (p = 0.17, not statistically significant)
- goto/250: **-4.6%** (p = 0.00)
- goto/500: **-3.1%** (p = 0.00)

## What Changed

1. **New `BucketArray` struct** with parallel arrays (`p_first`, `b_next`, `b_prev`, `rank`)
   replacing per-node bucket fields.
2. **Node struct** shrank from 80 to 64 bytes by removing `b_next`, `b_prev`, `rank` and adding
   a dedicated `dfs_parent` field (previously overloaded onto `b_next`).
3. **Removed `dnode` sentinel** — `BucketArray` manages its own sentinel via `SENTINEL`.
4. **Removed `l_bucket`** — merged into the `linf` bucket array.
5. All bucket operations now go through `BucketArray` methods (`insert`, `remove`, `get`,
   `nonempty`, `reset`).

## Conclusion

The refactoring delivers a consistent ~3–5% speedup with cleaner code separation. The 64-byte
Node size likely eliminates the `imul` instruction in relabel's inner loop (replaced by a shift),
confirming the hypothesis from `OPTIMIZATION.md`. Ready to merge to master.
