# Refactoring Opportunities

Potential refactors for `McmfCs2` in `src/lib.rs`, focused on making the Bucket
data structure more idiomatic in Rust.

## 1. Extract a `BucketArray` type

**Priority: High**

Currently, bucket operations (`insert_to_bucket`, `remove_from_bucket`,
`get_from_bucket`, `reset_bucket`, `nonempty_bucket`) are methods on `McmfCs2`
that reach across `self.buckets`, `self.nodes[i].b_next`, `self.nodes[i].b_prev`,
`self.nodes[i].rank`, and `self.dnode`. This couples bucket logic to the entire
solver and makes isolated testing impossible.

**Proposed change:** Extract a self-contained `BucketArray` struct that owns:

- `buckets: Vec<NodeIndex>` — the `p_first` head pointer per bucket
- `b_next: Vec<NodeIndex>` — per-node next pointer (parallel array)
- `b_prev: Vec<NodeIndex>` — per-node prev pointer (parallel array)
- `rank: Vec<i64>` — per-node bucket membership (parallel array)

The bucket membership fields (`b_next`, `b_prev`, `rank`) move from `Node` into
parallel arrays owned by `BucketArray`.

**Benefits:**

- Isolated unit testing of bucket insert/remove/get
- Cleaner API: `self.buckets.insert(node, bucket)` vs `self.insert_to_bucket(node, bucket)`
- `Node` shrinks from 80 to 56 bytes (removes `b_next`, `b_prev`, `rank`)

**Risk:**

- Bucket scans (e.g., `up_node_scan`) currently access `node.price` and
  `node.rank` from the same struct. With parallel arrays, `rank` is a separate
  allocation, potentially hurting cache locality. Needs benchmarking.

## 2. Replace the `dnode` sentinel with `NONE`

**Priority: Medium** (naturally falls out of #1)

The current design appends a real `Node` to the `nodes` array during
`cs2_initialize` solely to use its index as a "null" for empty bucket lists.
This is a C idiom that leaks bucket concerns into the node array and makes
`nodes.len()` unpredictable (it grows by 2 extra nodes: `dnode` + `dummy_node`).

**Proposed change:** Use `NONE` (`usize::MAX`) as the empty-bucket sentinel,
matching the convention already used for `q_next`, `b_next`, `first`, etc.
throughout the codebase. If extracted into `BucketArray`, the sentinel becomes
an internal detail.

**Benefits:**

- Removes `dnode` field from `McmfCs2`
- Stops `cs2_initialize` from pushing an extra node onto the array
- Consistent sentinel convention across the codebase

**Risk:** Minimal. `NONE` is already the null convention everywhere else.

## 3. Newtype indices

**Priority: Low**

`BucketIndex`, `NodeIndex`, and `ArcIndex` are all `type` aliases for `usize`.
The compiler cannot catch a `NodeIndex` passed where a `BucketIndex` is expected.

**Proposed change:** Replace with newtype wrappers:

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
struct NodeIdx(u32);
```

**Benefits:**

- Compile-time prevention of index misuse
- Could use `u32` instead of `usize`, halving index storage

**Risk:**

- High churn: pervasive changes across the entire codebase
- Arithmetic on newtypes requires `impl Add`, `impl From`, etc.
- Should only be attempted after higher-priority refactors are stable
