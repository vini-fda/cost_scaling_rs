//! Index-list ADT used by NETGEN to track available arc endpoints.
//!
//! Faithful port of `index.c` from the DIMACS NETGEN sources. The ADT supports:
//! - [`IndexList::new`] — build a list of consecutive integers `[from, to]`.
//! - [`IndexList::choose`] — remove and return the k'th smallest remaining
//!   element (1-indexed). Returns `None` for out-of-range positions.
//! - [`IndexList::remove`] — remove a specific value, decrementing
//!   [`IndexList::pseudo_size`] regardless of whether the value was present.
//! - [`IndexList::size`] / [`IndexList::pseudo_size`].
//!
//! The `pseudo_size` quirk (the list's reported "almost size" being decremented
//! even when `remove` finds nothing) is intentional: NETGEN's `pick_head`
//! routine depends on it. The C source explicitly marks it as "an apparent bug
//! in the original definition of the NETGEN program" that we must perpetuate.
//!
//! Two internal backends, switched by [`FLAG_LIMIT`]:
//! - **Flag array** (`original_size ≤ 100`): linear scan.
//! - **Binary interval tree** (`original_size > 100`): logarithmic per op,
//!   matching the C implementation.

/// Threshold below which we use the linear flag-array backend, matching the C
/// reference's `FLAG_LIMIT`.
const FLAG_LIMIT: u64 = 100;

#[derive(Debug)]
struct INode {
    base: u64,
    count: u64,
    /// `None` for leaves. When set, the right sibling lives at `left_child + 1`.
    left_child: Option<usize>,
}

#[derive(Debug)]
enum Backend {
    Flag { base: u64, flags: Vec<bool> },
    Tree { nodes: Vec<INode> },
}

#[derive(Debug)]
pub(crate) struct IndexList {
    index_size: u64,
    pseudo_size: u64,
    backend: Backend,
}

impl IndexList {
    /// Build an index list covering `[from, to]` inclusive.
    ///
    /// `from < 1` or `from > to` produces a permanently empty list, matching
    /// the C reference's behavior of returning an `-1` handle whose subsequent
    /// operations are no-ops.
    pub(crate) fn new(from: u64, to: u64) -> Self {
        if from < 1 || from > to {
            return Self {
                index_size: 0,
                pseudo_size: 0,
                backend: Backend::Flag {
                    base: from.max(1),
                    flags: Vec::new(),
                },
            };
        }
        let original_size = to - from + 1;
        let backend = if original_size <= FLAG_LIMIT {
            Backend::Flag {
                base: from,
                flags: vec![false; original_size as usize],
            }
        } else {
            Backend::Tree {
                nodes: vec![INode {
                    base: from,
                    count: original_size,
                    left_child: None,
                }],
            }
        };
        Self {
            index_size: original_size,
            pseudo_size: original_size,
            backend,
        }
    }

    /// Number of integers remaining in the list.
    pub(crate) fn size(&self) -> u64 {
        self.index_size
    }

    /// "Pseudo size": [`size`] minus the number of failed `remove` calls.
    ///
    /// Required by NETGEN's [`pick_head`] heuristic. See module docs.
    ///
    /// [`size`]: Self::size
    pub(crate) fn pseudo_size(&self) -> u64 {
        self.pseudo_size
    }

    /// Remove and return the integer at 1-indexed position `position` in the
    /// list of remaining integers. Returns `None` if `position` is out of
    /// range (matching the C version's 0 return).
    pub(crate) fn choose(&mut self, position: u64) -> Option<u64> {
        if position < 1 || position > self.index_size {
            return None;
        }
        self.index_size -= 1;
        self.pseudo_size -= 1;
        match &mut self.backend {
            Backend::Flag { base, flags } => Some(choose_flag(*base, flags, position)),
            Backend::Tree { nodes } => Some(choose_tree(nodes, position)),
        }
    }

    /// Remove a specific value from the list. `pseudo_size` is decremented
    /// regardless of whether the value was present; `size` only when it was.
    ///
    /// `pseudo_size` is allowed to wrap below zero — NETGEN intentionally
    /// surfaces the wrapped value (the C source casts it to signed `long`)
    /// to subsequent `random(1, pseudo_size)` calls. We use [`u64::wrapping_sub`]
    /// to reproduce that, and rely on callers casting back via `as i64`.
    pub(crate) fn remove(&mut self, value: u64) {
        self.pseudo_size = self.pseudo_size.wrapping_sub(1);
        let removed = match &mut self.backend {
            Backend::Flag { base, flags } => remove_flag(*base, flags, value),
            Backend::Tree { nodes } => remove_tree(nodes, value),
        };
        if removed {
            self.index_size -= 1;
        }
    }
}

fn choose_flag(base: u64, flags: &mut [bool], position: u64) -> u64 {
    let mut remaining = position;
    for (i, f) in flags.iter_mut().enumerate() {
        if !*f {
            remaining -= 1;
            if remaining == 0 {
                *f = true;
                return base + i as u64;
            }
        }
    }
    // Unreachable when called via `choose`, which has already range-checked
    // `position` against `index_size`. In tests we trip a debug assert; in
    // release we return `base` as a defensive fallback.
    debug_assert!(false, "choose_flag walked past end of flags vector");
    base
}

fn remove_flag(base: u64, flags: &mut [bool], value: u64) -> bool {
    if value < base || value >= base + flags.len() as u64 {
        return false;
    }
    let idx = (value - base) as usize;
    if flags[idx] {
        false
    } else {
        flags[idx] = true;
        true
    }
}

fn choose_tree(nodes: &mut Vec<INode>, mut position: u64) -> u64 {
    let mut np = 0usize;
    while let Some(lc) = nodes[np].left_child {
        nodes[np].count -= 1;
        let left_count = nodes[lc].count;
        np = if position > left_count {
            position -= left_count;
            lc + 1
        } else {
            lc
        };
    }
    nodes[np].count -= 1;
    let leaf_base = nodes[np].base;
    let leaf_count_after = nodes[np].count;
    if position == 1 {
        nodes[np].base += 1;
        leaf_base
    } else if position > leaf_count_after {
        leaf_base + leaf_count_after
    } else {
        let index = leaf_base + position - 1;
        let count_l = position - 1;
        let count_r = leaf_count_after - count_l;
        let new_left_idx = nodes.len();
        nodes[np].left_child = Some(new_left_idx);
        nodes.push(INode {
            base: leaf_base,
            count: count_l,
            left_child: None,
        });
        nodes.push(INode {
            base: index + 1,
            count: count_r,
            left_child: None,
        });
        index
    }
}

fn remove_tree(nodes: &mut Vec<INode>, target: u64) -> bool {
    // Locate the leaf interval that would contain `target`, matching the C
    // descent (right-child first, back off to left when the right base is
    // already past the target).
    let mut np = 0usize;
    let mut path: Vec<usize> = Vec::new();
    while let Some(lc) = nodes[np].left_child {
        path.push(np);
        let right = lc + 1;
        np = if target < nodes[right].base {
            lc
        } else {
            right
        };
    }
    let leaf_base = nodes[np].base;
    let leaf_count = nodes[np].count;
    if target < leaf_base || target >= leaf_base + leaf_count {
        return false;
    }
    // Found — apply the decrement up the path.
    for &p in &path {
        nodes[p].count -= 1;
    }
    nodes[np].count -= 1;
    let leaf_count_after = nodes[np].count;
    if target == leaf_base {
        nodes[np].base += 1;
    } else if target == leaf_base + leaf_count_after {
        // Trailing element — already covered by the count decrement.
    } else {
        let count_l = target - leaf_base;
        let count_r = leaf_count_after - count_l;
        let new_left_idx = nodes.len();
        nodes[np].left_child = Some(new_left_idx);
        nodes.push(INode {
            base: leaf_base,
            count: count_l,
            left_child: None,
        });
        nodes.push(INode {
            base: target + 1,
            count: count_r,
            left_child: None,
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_in_order(list: &mut IndexList) -> Vec<u64> {
        let mut out = Vec::new();
        while list.size() > 0 {
            out.push(list.choose(1).expect("size > 0 implies choose(1) is Some"));
        }
        out
    }

    #[test]
    fn flag_backend_simple_drain() {
        let mut list = IndexList::new(5, 10);
        assert_eq!(list.size(), 6);
        assert_eq!(list.pseudo_size(), 6);
        assert_eq!(drain_in_order(&mut list), vec![5, 6, 7, 8, 9, 10]);
        assert_eq!(list.size(), 0);
        assert_eq!(list.choose(1), None);
    }

    #[test]
    fn flag_backend_choose_middle() {
        let mut list = IndexList::new(1, 10);
        assert_eq!(list.choose(3), Some(3));
        assert_eq!(list.choose(3), Some(4));
        assert_eq!(list.choose(1), Some(1));
        assert_eq!(list.size(), 7);
    }

    #[test]
    fn flag_backend_remove_present_and_absent() {
        let mut list = IndexList::new(1, 10);
        list.remove(5);
        assert_eq!(list.size(), 9);
        assert_eq!(list.pseudo_size(), 9);
        // Removing 5 again: not present, but pseudo_size still decrements.
        list.remove(5);
        assert_eq!(list.size(), 9);
        assert_eq!(list.pseudo_size(), 8);
        // Removing an out-of-range value: same quirk.
        list.remove(999);
        assert_eq!(list.size(), 9);
        assert_eq!(list.pseudo_size(), 7);
    }

    #[test]
    fn tree_backend_simple_drain() {
        let mut list = IndexList::new(1, 200);
        assert_eq!(list.size(), 200);
        let drained = drain_in_order(&mut list);
        assert_eq!(drained, (1..=200).collect::<Vec<_>>());
    }

    #[test]
    fn tree_backend_choose_random_positions() {
        let mut list = IndexList::new(1, 200);
        // Equivalent C behavior: choose returns the k'th smallest currently
        // present. Removing pos=100 (=100), then pos=1 (=1), then pos=99
        // (which is currently 100 because index 100 was removed — but wait,
        // after removing 100 the remaining are 1..=99 ++ 101..=200, so pos=99
        // is 99; then after removing 1, pos=99 in [2..=99, 101..=200] is 100).
        assert_eq!(list.choose(100), Some(100));
        assert_eq!(list.choose(1), Some(1));
        assert_eq!(list.choose(99), Some(101));
        assert_eq!(list.size(), 197);
    }

    #[test]
    fn tree_backend_remove_then_choose() {
        let mut list = IndexList::new(1, 200);
        list.remove(150);
        assert_eq!(list.size(), 199);
        // Choose pos=150 should now return 151 (since 150 is gone).
        assert_eq!(list.choose(150), Some(151));
        assert_eq!(list.size(), 198);
    }

    #[test]
    fn tree_backend_remove_absent_only_touches_pseudo_size() {
        let mut list = IndexList::new(1, 200);
        list.remove(1000); // out of range
        assert_eq!(list.size(), 200);
        assert_eq!(list.pseudo_size(), 199);
        list.remove(50);
        assert_eq!(list.size(), 199);
        assert_eq!(list.pseudo_size(), 198);
        list.remove(50); // already gone
        assert_eq!(list.size(), 199);
        assert_eq!(list.pseudo_size(), 197);
    }

    #[test]
    fn tree_backend_repeated_splits() {
        // Cause many splits by choosing interior positions on a long interval.
        let mut list = IndexList::new(1, 1000);
        for _ in 0..50 {
            let pos = list.size() / 2;
            list.choose(pos);
        }
        assert_eq!(list.size(), 950);
    }
}
