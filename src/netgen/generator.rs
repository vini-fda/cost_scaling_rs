//! NETGEN main generator: 4-phase network construction.
//!
//! Faithful Rust port of the C `netgen()` routine in `netgen.c`. The function
//! and variable names follow the C source so that the two are easy to diff.
//!
//! Generates:
//! - an assignment problem when `(SOURCES − TSOURCES) + (SINKS − TSINKS) == NODES`,
//!   `(SOURCES − TSOURCES) == (SINKS − TSINKS)`, and `SOURCES == SUPPLY`;
//! - a max-flow problem when `MINCOST == MAXCOST == 1` (and not an assignment);
//! - a min-cost flow problem otherwise.
//!
//! The four phases are:
//! 1. Distribute total supply across the source nodes (`create_supply`).
//! 2. Build a forest of source-rooted "chains" through the transshipment
//!    nodes via the `pred[]` linked list.
//! 3. For each chain, pick sinks, distribute the chain's supply, place
//!    skeleton arcs, and sprinkle "rubbish" (chord) arcs via `pick_head`.
//! 4. Add more rubbish arcs out of every transshipment sink.

use std::fmt;

use super::index_list::IndexList;
use super::rng::Rng;

/// The 13 NETGEN parameters, in canonical order.
#[derive(Debug, Clone, Copy)]
pub struct NetgenParams {
    /// Total number of nodes.
    pub nodes: u64,
    /// Number of sources (including transshipment sources).
    pub sources: u64,
    /// Number of sinks (including transshipment sinks).
    pub sinks: u64,
    /// Requested number of arcs.
    pub density: u64,
    /// Minimum arc cost.
    pub min_cost: i64,
    /// Maximum arc cost.
    pub max_cost: i64,
    /// Total supply across all sources.
    pub supply: i64,
    /// Number of transshipment sources.
    pub t_sources: u64,
    /// Number of transshipment sinks.
    pub t_sinks: u64,
    /// Percentage of skeleton arcs given maximum cost.
    pub hi_cost: u64,
    /// Percentage of arcs that should be capacitated.
    pub capacitated: u64,
    /// Minimum capacity for capacitated arcs.
    pub min_cap: i64,
    /// Maximum capacity for capacitated arcs.
    pub max_cap: i64,
}

/// Kind of DIMACS problem NETGEN emits, derived from the parameter combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    /// `p min`
    MinCostFlow,
    /// `p max` — emitted when not an assignment and `min_cost == max_cost == 1`.
    MaxFlow,
    /// `p asn`
    Assignment,
}

/// A generated NETGEN instance.
#[derive(Debug, Clone)]
pub struct NetgenInstance {
    /// Parameters used to generate this instance (echoed for the comment
    /// header).
    pub params: NetgenParams,
    /// Random seed used.
    pub seed: i64,
    /// Per-node supply (positive) or demand (negative). Length == `nodes`.
    pub supply: Vec<i64>,
    /// Arc tails (1-indexed).
    pub from: Vec<u64>,
    /// Arc heads (1-indexed).
    pub to: Vec<u64>,
    /// Arc capacities.
    pub cap: Vec<i64>,
    /// Arc costs.
    pub cost: Vec<i64>,
    /// Detected problem kind.
    pub kind: ProblemKind,
}

/// Errors NETGEN can raise.
#[derive(Debug, PartialEq, Eq)]
pub enum NetgenError {
    /// Seed must be positive.
    BadSeed,
    /// Inconsistent or out-of-range parameters.
    BadParams(&'static str),
}

impl fmt::Display for NetgenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetgenError::BadSeed => write!(f, "NETGEN requires a positive random seed"),
            NetgenError::BadParams(msg) => write!(f, "inconsistent parameter settings: {msg}"),
        }
    }
}

impl std::error::Error for NetgenError {}

// Shared mutable state used across the four phases. Mirrors the C `static`
// globals in `netgen.c` but scoped to a single `generate` call so it is
// thread-safe and reentrant.
struct Builder {
    params: NetgenParams,
    seed: i64,
    rng: Rng,
    /// 0-indexed supply array (length `nodes`).
    supply: Vec<i64>,
    /// Emitted arc tails (1-indexed).
    from: Vec<u64>,
    /// Emitted arc heads (1-indexed).
    to: Vec<u64>,
    /// Emitted arc capacities.
    cap: Vec<i64>,
    /// Emitted arc costs.
    cost: Vec<i64>,
    /// Per-node chain links (1-indexed, size `nodes + 1`).
    pred: Vec<u64>,
    /// Per-source scratch: skeleton heads (1-indexed, size `nodes + 2`).
    head_buf: Vec<u64>,
    /// Per-source scratch: skeleton tails (1-indexed, size `nodes + 2`).
    tail_buf: Vec<u64>,
    /// Non-pure-sink count remaining; decremented in each `pick_head` call.
    nodes_left: u64,
}

impl Builder {
    fn new(seed: i64, params: NetgenParams) -> Self {
        let n = params.nodes as usize;
        Self {
            params,
            seed,
            rng: Rng::new(seed),
            supply: vec![0; n],
            from: Vec::new(),
            to: Vec::new(),
            cap: Vec::new(),
            cost: Vec::new(),
            pred: vec![0; n + 2],
            head_buf: vec![0; n + 2],
            tail_buf: vec![0; n + 2],
            nodes_left: 0,
        }
    }

    #[inline]
    fn save_arc(&mut self, tail: u64, head: u64, cost: i64, cap: i64) {
        self.from.push(tail);
        self.to.push(head);
        self.cost.push(cost);
        self.cap.push(cap);
    }

    #[inline]
    fn arc_count(&self) -> u64 {
        self.from.len() as u64
    }

    /// Phase 1: distribute total supply across source nodes.
    /// Mirrors `create_supply` in `netgen.c`.
    fn create_supply(&mut self, sources: u64, supply: i64) {
        let supply_per_source = supply / sources as i64;
        for i in 0..sources as usize {
            let partial_supply = self.rng.random(1, supply_per_source);
            self.supply[i] += partial_supply;
            let j = self.rng.random(0, sources as i64 - 1) as usize;
            self.supply[j] += supply_per_source - partial_supply;
        }
        let j = self.rng.random(0, sources as i64 - 1) as usize;
        self.supply[j] += supply % sources as i64;
    }

    /// Phase 3 helper: append `limit` "rubbish" arcs from `desired_tail` to
    /// randomly chosen heads drawn from `handle`. Mirrors `pick_head` in
    /// `netgen.c`, including the wrap-around arithmetic that arises when the
    /// arc count has overshot `density` (which the C reference relies on:
    /// `remaining_arcs` wraps to a huge unsigned value, and when assigned
    /// back to `int limit`, truncates to a negative integer that short-circuits
    /// the rubbish-arc loop).
    fn pick_head(&mut self, handle: &mut IndexList, desired_tail: u64) {
        let p = self.params;
        let non_sources = p.nodes - p.sources + p.t_sources;
        // C: `ARC remaining_arcs = DENSITY - arc_count;` (unsigned long).
        let remaining_arcs: u64 = p.density.wrapping_sub(self.arc_count());

        // C: `nodes_left--;` (unsigned long).
        self.nodes_left = self.nodes_left.wrapping_sub(1);
        if self.nodes_left.wrapping_mul(2) >= remaining_arcs {
            return;
        }

        // Compute `limit` as the C does: mostly unsigned-long arithmetic, with
        // a critical truncation to `int` at the end of the else branch.
        let pseudo: u64 = handle.pseudo_size();
        let lhs: u64 = remaining_arcs
            .wrapping_add(non_sources)
            .wrapping_sub(pseudo)
            .wrapping_sub(1)
            / self.nodes_left.wrapping_add(1);
        let limit: i32 = if lhs >= non_sources.wrapping_sub(1) {
            // C: `limit = non_sources;` — truncates `unsigned long → int`.
            non_sources as i32
        } else {
            // C: `upper_bound = 2 * (remaining_arcs / (nodes_left + 1) - 1);`
            // computed in `long`. When `remaining_arcs` has wrapped, the
            // signed cast yields a negative `upper_bound`, which makes
            // `random(1, upper_bound)` return `upper_bound` unchanged.
            let upper_bound: i64 = ((remaining_arcs / self.nodes_left.wrapping_add(1)) as i64)
                .wrapping_sub(1)
                .wrapping_mul(2);
            loop {
                let mut chosen: i32 = self.rng.random(1, upper_bound) as i32;
                if self.nodes_left == 0 {
                    // C: `limit = remaining_arcs;` — `unsigned long → int`
                    // truncation. When `remaining_arcs` has wrapped, this
                    // becomes a small negative number that short-circuits the
                    // outer for loop below.
                    chosen = remaining_arcs as i32;
                }
                let condition_lhs: u64 = self.nodes_left.wrapping_mul(non_sources.wrapping_sub(1));
                let condition_rhs: u64 = remaining_arcs.wrapping_sub(i64::from(chosen) as u64);
                if condition_lhs >= condition_rhs {
                    break chosen;
                }
            }
        };

        let iters: u64 = if limit > 0 { limit as u64 } else { 0 };
        for _ in 0..iters {
            // `pseudo_size` may have wrapped: cast through `i64` so a wrapped
            // value (the "perpetuated bug") becomes negative, matching
            // C's `(long)pseudo_size(handle)`.
            let pos_signed = self.rng.random(1, handle.pseudo_size() as i64);
            let pos = pos_signed as u64;
            let index = handle.choose(pos).unwrap_or(0);
            let mut cap = p.supply;
            if self.rng.random(1, 100) <= p.capacitated as i64 {
                cap = self.rng.random(p.min_cap, p.max_cap);
            }
            let cost = self.rng.random(p.min_cost, p.max_cost);
            self.save_arc(desired_tail, index, cost, cap);
        }
    }
}

/// Shell-sort `head_buf[1..=sort_count]` in lockstep with `tail_buf` by
/// `tail_buf` values. Mirrors `sort_skeleton` in `netgen.c`.
fn sort_skeleton(head_buf: &mut [u64], tail_buf: &mut [u64], sort_count: u64) {
    let mut m = sort_count;
    while {
        m /= 2;
        m != 0
    } {
        let k = sort_count - m;
        for j in 1..=k {
            let mut i = j;
            while i >= 1 && tail_buf[i as usize] > tail_buf[(i + m) as usize] {
                tail_buf.swap(i as usize, (i + m) as usize);
                head_buf.swap(i as usize, (i + m) as usize);
                if i < m {
                    break;
                }
                i -= m;
            }
        }
    }
}

/// Generate an assignment instance. Mirrors `create_assignment`.
fn create_assignment(b: &mut Builder) -> Result<(), NetgenError> {
    let n = b.params.nodes;
    let half = n / 2;
    for s in 0..half as usize {
        b.supply[s] = 1;
    }
    for s in half as usize..n as usize {
        b.supply[s] = -1;
    }

    let mut skeleton = IndexList::new(b.params.sources + 1, n);
    for source in 1..=half {
        let pos = b.rng.random(1, skeleton.size() as i64) as u64;
        let index = skeleton.choose(pos).unwrap_or(0);
        let cost = b.rng.random(b.params.min_cost, b.params.max_cost);
        b.save_arc(source, index, cost, 1);

        let mut handle = IndexList::new(b.params.sources + 1, n);
        handle.remove(index);
        b.pick_head(&mut handle, source);
    }
    Ok(())
}

/// Generate a NETGEN instance for the given seed and parameters.
///
/// Returns a [`NetgenInstance`] whose arcs, supplies, and parameter echo match
/// the C reference binary byte-for-byte when emitted through
/// [`write_dimacs`].
///
/// [`write_dimacs`]: super::write_dimacs
pub fn generate(seed: i64, params: NetgenParams) -> Result<NetgenInstance, NetgenError> {
    validate(seed, &params)?;

    let mut b = Builder::new(seed, params);

    // Compute nodes_left and the assignment-special-case predicate.
    b.nodes_left = params.nodes - params.sinks + params.t_sinks;

    let is_assignment = (params.sources - params.t_sources) + (params.sinks - params.t_sinks)
        == params.nodes
        && (params.sources - params.t_sources) == (params.sinks - params.t_sinks)
        && params.sources as i64 == params.supply;

    if is_assignment {
        create_assignment(&mut b)?;
        return Ok(finalize(b, ProblemKind::Assignment));
    }

    // -------- Phase 1: assign supply across sources. --------
    b.create_supply(params.sources, params.supply);

    // -------- Phase 2: build the skeleton chain forest. --------
    // pred[i] forms a circular linked list per source: walking pred from
    // `source` returns to `source` after visiting every node in that chain.
    for i in 1..=params.sources as usize {
        b.pred[i] = i as u64;
    }
    let mut handle = IndexList::new(params.sources + 1, params.nodes - params.sinks);
    let trans = params.nodes - params.sources - params.sinks;
    // 60% of transshipment nodes go round-robin onto sources; the rest pile
    // onto random sources. The threshold `(4t + 9) / 10` (= `ceil(4t / 10)`)
    // is what the C uses.
    let threshold = (4 * trans).div_ceil(10);
    let mut source = 1u64;
    let mut i = trans;
    while i > threshold {
        let node = handle
            .choose(b.rng.random(1, handle.size() as i64) as u64)
            .unwrap_or(0);
        b.pred[node as usize] = b.pred[source as usize];
        b.pred[source as usize] = node;
        source += 1;
        if source > params.sources {
            source = 1;
        }
        i -= 1;
    }
    while i > 0 {
        let node = handle
            .choose(b.rng.random(1, handle.size() as i64) as u64)
            .unwrap_or(0);
        let rs = b.rng.random(1, params.sources as i64) as u64;
        b.pred[node as usize] = b.pred[rs as usize];
        b.pred[rs as usize] = node;
        i -= 1;
    }
    drop(handle);

    // -------- Phase 3: hook each source chain to sinks and emit arcs. --------
    for source in 1..=params.sources {
        let mut sort_count: u64 = 0;
        // Walk the chain into head_buf/tail_buf.
        let mut node = b.pred[source as usize];
        while node != source {
            sort_count += 1;
            b.head_buf[sort_count as usize] = node;
            let next = b.pred[node as usize];
            b.tail_buf[sort_count as usize] = next;
            node = next;
        }

        // Sinks for this chain.
        let mut sinks_per_source: u64 = if params.nodes - params.sources - params.sinks == 0 {
            params.sinks / params.sources + 1
        } else {
            // i128 for safety on huge problems (bcjl overflow fix).
            ((2 * i128::from(sort_count) * i128::from(params.sinks))
                / i128::from(params.nodes - params.sources - params.sinks)) as u64
        };
        sinks_per_source = sinks_per_source.clamp(2, params.sinks);

        let mut handle = IndexList::new(params.nodes - params.sinks, params.nodes - 1);
        let mut sinks: Vec<u64> = Vec::with_capacity(sinks_per_source as usize);
        for _ in 0..sinks_per_source {
            let pos = b.rng.random(1, handle.size() as i64) as u64;
            let chosen = handle.choose(pos).unwrap_or(0);
            sinks.push(chosen);
        }
        // Last source mops up any remaining sinks the chain hasn't touched
        // (provided their supply is still 0, i.e. they haven't been claimed by
        // a previous chain via the partial-supply scatter).
        if source == params.sources && handle.size() > 0 {
            while handle.size() > 0 {
                let j = handle.choose(1).unwrap_or(0);
                if b.supply[j as usize] == 0 {
                    sinks.push(j);
                    sinks_per_source += 1;
                }
            }
        }
        drop(handle);

        let chain_length = sort_count;
        let source_supply = b.supply[source as usize - 1];
        let supply_per_sink = source_supply / sinks_per_source as i64;
        let mut k = b.pred[source as usize];
        for i in 0..sinks_per_source as usize {
            sort_count += 1;
            let partial_supply = b.rng.random(1, supply_per_sink);
            let j = b.rng.random(0, sinks_per_source as i64 - 1) as usize;
            b.tail_buf[sort_count as usize] = k;
            b.head_buf[sort_count as usize] = sinks[i] + 1;
            b.supply[sinks[i] as usize] -= partial_supply;
            b.supply[sinks[j] as usize] -= supply_per_sink - partial_supply;
            k = source;
            let mut hops = b.rng.random(1, chain_length as i64);
            while hops > 0 {
                k = b.pred[k as usize];
                hops -= 1;
            }
        }
        b.supply[sinks[0] as usize] -= source_supply % sinks_per_source as i64;

        // Sort skeleton arcs by tail, then emit them + a pick_head per tail.
        sort_skeleton(&mut b.head_buf, &mut b.tail_buf, sort_count);
        b.tail_buf[sort_count as usize + 1] = 0; // sentinel

        let mut i_idx: u64 = 1;
        while i_idx <= sort_count {
            let mut handle = IndexList::new(params.sources - params.t_sources + 1, params.nodes);
            handle.remove(b.tail_buf[i_idx as usize]);
            let it = b.tail_buf[i_idx as usize];
            while b.tail_buf[i_idx as usize] == it {
                handle.remove(b.head_buf[i_idx as usize]);
                let mut cap = params.supply;
                if b.rng.random(1, 100) <= params.capacitated as i64 {
                    cap = source_supply.max(params.min_cap);
                }
                let mut cost = params.max_cost;
                if b.rng.random(1, 100) > params.hi_cost as i64 {
                    cost = b.rng.random(params.min_cost, params.max_cost);
                }
                b.save_arc(it, b.head_buf[i_idx as usize], cost, cap);
                i_idx += 1;
            }
            b.pick_head(&mut handle, it);
        }
    }

    // -------- Phase 4: rubbish edges out of every transshipment sink. --------
    for i in (params.nodes - params.sinks + 1)..=(params.nodes - params.sinks + params.t_sinks) {
        let mut handle = IndexList::new(params.sources - params.t_sources + 1, params.nodes);
        handle.remove(i);
        b.pick_head(&mut handle, i);
    }

    let kind = if params.min_cost == 1 && params.max_cost == 1 {
        ProblemKind::MaxFlow
    } else {
        ProblemKind::MinCostFlow
    };
    Ok(finalize(b, kind))
}

fn finalize(b: Builder, kind: ProblemKind) -> NetgenInstance {
    NetgenInstance {
        params: b.params,
        seed: b.seed,
        supply: b.supply,
        from: b.from,
        to: b.to,
        cap: b.cap,
        cost: b.cost,
        kind,
    }
}

fn validate(seed: i64, p: &NetgenParams) -> Result<(), NetgenError> {
    if seed <= 0 {
        return Err(NetgenError::BadSeed);
    }
    if p.nodes == 0 || p.nodes > p.density {
        return Err(NetgenError::BadParams(
            "nodes must satisfy 1 <= nodes <= density",
        ));
    }
    if p.sources == 0 {
        return Err(NetgenError::BadParams("sources must be positive"));
    }
    if p.sinks == 0 {
        return Err(NetgenError::BadParams("sinks must be positive"));
    }
    if p.sources + p.sinks > p.nodes {
        return Err(NetgenError::BadParams("sources + sinks > nodes"));
    }
    if p.min_cost > p.max_cost {
        return Err(NetgenError::BadParams("min_cost > max_cost"));
    }
    if p.supply < p.sources as i64 {
        return Err(NetgenError::BadParams("supply < sources"));
    }
    if p.t_sources > p.sources {
        return Err(NetgenError::BadParams("t_sources > sources"));
    }
    if p.t_sinks > p.sinks {
        return Err(NetgenError::BadParams("t_sinks > sinks"));
    }
    if p.hi_cost > 100 {
        return Err(NetgenError::BadParams("hi_cost > 100"));
    }
    if p.capacitated > 100 {
        return Err(NetgenError::BadParams("capacitated > 100"));
    }
    if p.min_cap > p.max_cap {
        return Err(NetgenError::BadParams("min_cap > max_cap"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem_1_params() -> NetgenParams {
        // Problem 1 from `problems.40`.
        NetgenParams {
            nodes: 200,
            sources: 100,
            sinks: 100,
            density: 1300,
            min_cost: 1,
            max_cost: 10_000,
            supply: 100_000,
            t_sources: 0,
            t_sinks: 0,
            hi_cost: 0,
            capacitated: 0,
            min_cap: 0,
            max_cap: 0,
        }
    }

    #[test]
    fn problem_1_summary_matches_c() {
        // The C reference reports `p min 200 1308` for problem 1 (seed
        // 13502460). Our port should generate the same arc count.
        let inst = generate(13_502_460, problem_1_params()).expect("generate");
        assert_eq!(inst.kind, ProblemKind::MinCostFlow);
        assert_eq!(inst.from.len(), 1308);
    }

    #[test]
    fn rejects_bad_seed() {
        assert_eq!(
            generate(0, problem_1_params()).expect_err("seed 0 must error"),
            NetgenError::BadSeed
        );
        assert_eq!(
            generate(-1, problem_1_params()).expect_err("negative seed must error"),
            NetgenError::BadSeed
        );
    }

    #[test]
    fn rejects_bad_params() {
        let mut p = problem_1_params();
        p.sources = p.nodes; // sources + sinks > nodes
        assert!(matches!(
            generate(1, p).expect_err("invalid params must error"),
            NetgenError::BadParams(_)
        ));
    }
}
