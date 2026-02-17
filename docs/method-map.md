# McmfCs2 Method-to-Paper Map

Maps each method in `McmfCs2` (defined in `src/lib.rs`) to its corresponding
concept in:

> Goldberg, A.V. "An Efficient Implementation of a Scaling Minimum-Cost Flow
> Algorithm." *Journal of Algorithms*, 22(1), 1997, pp. 1–29.

## Main algorithm

| Method | Paper concept | Section |
|--------|--------------|---------|
| `cs2` | Successive approximation main loop | §1, Fig 1 (Min-Cost) |
| `refine` | FIFO push-relabel to convert epsilon-optimal pseudoflow into (epsilon/alpha)-optimal flow | §1, Fig 2 (Refine) |
| `discharge` | Push/relabel on a single active node until inactive | §1, Fig 4 (Discharge) |
| `relabel` | Scan residual arcs, update node price to best admissible value | §1, Fig 3 (Relabel) |
| `increase_flow` | Push operation: send flow along an admissible arc | §1, Fig 3 (Push) |
| `update_epsilon` | Reduce epsilon by scale factor alpha for the next scaling phase | §1 |

## Heuristics

| Method | Paper concept | Section |
|--------|--------------|---------|
| `price_update` | Global price recomputation via Dijkstra-like bucket scan from deficit nodes | §2.1 (Price updates) |
| `up_node_scan` | Per-node scan during price_update: propagate distance labels through reverse residual arcs | §2.1 |
| `price_refine` | Modify prices (without changing flow) to establish epsilon-optimality; DFS topo-sort + longest-path bucket scan; cancels negative cycles if found | §2.2 (Price refinement) |
| `price_out` | Speculative arc fixing: suspend arcs with large reduced cost so relabel/discharge skip them | §2.3 (Arc fixing) |
| `price_in` | Reverse of price_out: unsuspend arcs whose reduced cost fell back within threshold; saturates "bad fix-ins" (admissible suspended arcs) | §2.3 |
| `update_cut_off` | Adjust the speculative arc-fixing threshold beta based on bad fix-in / bad relabel counts | §2.3 |

The **push lookahead** heuristic (§2.4) is embedded inside `discharge`: before
pushing to a node `j` with non-negative excess, it checks whether `j` has an
outgoing admissible arc. If `j` transitions from zero excess to positive excess
from the push, `j` is relabeled immediately to avoid the common pattern where
flow is pushed right back to the sender.

## Initialization and post-processing

| Method | Paper concept |
|--------|--------------|
| `pre_processing` | Reorder arcs by source node (prefix-sum permutation), shift to zero-based indices |
| `cs2_initialize` | Saturate negative-cost arcs, scale costs by `dn = n+1`, allocate Dial buckets |
| `finishup` | Unscale costs/prices back to original units, compute objective cost |
| `compute_prices` | Compute final dual prices (node potentials) via DFS topo-sort + longest-path bucket scan over all arcs (including suspended) |

## Solution verification

| Method | Paper concept |
|--------|--------------|
| `is_feasible` | Check flow conservation and capacity constraints |
| `check_cs` | Check complementary slackness: no residual arc has negative reduced cost |

## Data structures

| Structure | Role |
|-----------|------|
| Excess queue (`excq_*`, `insert_to_excess_q`, etc.) | FIFO queue of active nodes for `refine` |
| Stack-queue (`stackq_push`, `stackq_pop`) | Reuses the excess queue as a LIFO stack for DFS finish-order in `price_refine` / `compute_prices` |
| Buckets (`buckets`, `insert_to_bucket`, etc.) | Dial-style doubly-linked bucket lists for `price_update` and `price_refine` |
| `exchange` | Swaps two arcs in the adjacency list, used by `price_out` / `price_in` to move arcs between active range `[first, suspended)` and suspended range `[suspended, first)` |

## Arc ranges per node

Each node's arcs in the `arcs` array are partitioned into two contiguous ranges:

```
[suspended ................. first ................. next_node.suspended)
 ^                           ^
 suspended (fixed) arcs      active arcs (scanned by relabel/discharge)
```

- `price_out` moves arcs from active to suspended (increments `first`).
- `price_in` moves arcs from suspended to active (decrements `first`).
