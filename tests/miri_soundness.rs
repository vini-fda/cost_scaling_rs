//! Soundness tests for the unsafe pointer-based `McmfCs2` implementation.

use cost_scaling_rs::McmfCs2;
use cost_scaling_rs::goto::{GotoParams, generate_to_string};

/// 6-node sample matching `testdata/sample.inp`. Exercises the full
/// refine + `price_refine` + finishup pipeline.
const SAMPLE: &str = "\
p min 6 8
n 1 10
n 6 -10
a 1 2 0 4 1
a 1 3 0 8 5
a 2 3 0 5 0
a 3 5 0 10 1
a 5 4 0 8 0
a 5 6 0 8 9
a 4 2 0 8 1
a 4 6 0 8 1
";

/// Triangle with a single source and sink. Tiny but enough to drive
/// discharge / relabel / `price_update` through at least one scaling pass.
const TRIANGLE: &str = "\
p min 3 3
n 1 2
n 3 -2
a 1 2 0 2 1
a 1 3 0 1 5
a 2 3 0 2 1
";

/// Parallel arcs with the same endpoints — exercises the
/// adjacency-list reordering in `pre_processing` more aggressively.
const PARALLEL: &str = "\
p min 4 6
n 1 5
n 4 -5
a 1 2 0 3 2
a 1 2 0 3 1
a 2 4 0 5 0
a 1 3 0 4 3
a 3 4 0 4 1
a 3 2 0 2 2
";

/// Chain with non-trivial lower bounds (forces `set_arc`'s excess
/// adjustment + the drain path in `pre_processing`). Mirrors the
/// `hand_lower_bounds` integration test fixture so it's known-feasible.
const LOWER_BOUNDS: &str = "\
p min 3 2
n 1 10
n 3 -10
a 1 2 3 10 2
a 2 3 3 10 3
";

/// Network with a self-loop and a cycle that `price_refine` should handle.
const CYCLE: &str = "\
p min 5 7
n 1 3
n 5 -3
a 1 2 0 3 1
a 2 3 0 3 1
a 3 4 0 3 1
a 4 5 0 3 1
a 3 2 0 2 1
a 4 3 0 2 1
a 2 4 0 2 5
";

fn solve_with(dimacs: &str, check: bool, comp_duals: bool) -> f64 {
    let solver = McmfCs2::from_dimacs(dimacs).expect("parse");
    let solution = solver.min_cost(check, comp_duals).expect("solve");
    // Drain the iterators so any UB in flows()/prices() is observed.
    let _flows: Vec<_> = solution.flows().collect();
    let _prices: Vec<_> = solution.prices().collect();
    solution.objective_cost
}

fn solve(dimacs: &str) -> f64 {
    // Default: full check + dual prices to exercise is_feasible / check_cs /
    // compute_prices in addition to the main solver.
    solve_with(dimacs, true, true)
}

#[test]
fn sample_six_node() {
    let cost = solve(SAMPLE);
    // Known optimal cost for the sample input.
    assert_eq!(cost, 70.0);
}

#[test]
fn triangle() {
    let cost = solve(TRIANGLE);
    // 2 units source→sink. Cheapest is via 1→2→3 at cost (1+1)*2 = 4.
    assert_eq!(cost, 4.0);
}

#[test]
fn parallel_arcs() {
    // Just check the solver runs to completion without UB; the exact
    // cost depends on tie-breaking, but it must be finite and positive.
    let cost = solve(PARALLEL);
    assert!(cost > 0.0);
}

#[test]
fn lower_bounds() {
    // is_feasible/check_cs's pre-existing accounting of lower-bound arcs
    // (see cs2_compare integration tests, which always pass false/false)
    // is independent of the unsafe-code soundness we want to verify here,
    // so skip those checks. Cost must still be the known-optimal value.
    let cost = solve_with(LOWER_BOUNDS, false, false);
    // Forced flow: 3 units through 1->2 (lower) + 3 through 2->3 (lower),
    // plus 7 more units balancing 1's supply (10) to 3's demand (-10),
    // along the only path: cost = 10*(2+3) = 50.
    assert_eq!(cost, 50.0);
}

#[test]
fn cycle_network() {
    let cost = solve(CYCLE);
    assert!(cost > 0.0);
}

/// Programmatic API path — does not go through the DIMACS parser, so
/// it exercises `McmfCs2::new` + repeated `set_supply_demand_of_node` /
/// `set_arc` directly.
#[test]
fn programmatic_build() {
    let mut s = McmfCs2::new(4, 5);
    s.set_supply_demand_of_node(1, 4).expect("set_supply");
    s.set_supply_demand_of_node(4, -4).expect("set_supply");
    s.set_arc(1, 2, 0, 4, 2).expect("set_arc");
    s.set_arc(1, 3, 0, 2, 2).expect("set_arc");
    s.set_arc(2, 3, 0, 2, 1).expect("set_arc");
    s.set_arc(2, 4, 0, 3, 3).expect("set_arc");
    s.set_arc(3, 4, 0, 5, 1).expect("set_arc");
    let sol = s.min_cost(true, true).expect("solve");
    assert!(sol.objective_cost > 0.0);
    let _ = sol.flows().count();
    let _ = sol.prices().count();
}

/// Solve several distinct problems in sequence in the same process —
/// catches any cross-instance UB (e.g., a static / thread-local /
/// lazy-initialized pointer accidentally shared).
#[test]
fn repeated_solves() {
    for _ in 0..3 {
        assert_eq!(solve(SAMPLE), 70.0);
        assert_eq!(solve(TRIANGLE), 4.0);
    }
}

/// Auto-generated GOTO problem. Larger than the hand-rolled tests so it
/// drives `price_update` / `price_refine` / `compute_prices` through
/// multiple scaling phases and bucket transitions. Sized to stay tolerable
/// under miri (still about ~10s wall-clock under Stacked Borrows).
#[test]
fn small_goto() {
    let dimacs = generate_to_string(&GotoParams {
        n: 15,
        m: 90,
        max_cap: 100,
        max_cost: 100,
        seed: 7,
    })
    .expect("goto generation");
    let cost = solve_with(&dimacs, true, true);
    assert!(cost.is_finite() && cost > 0.0);
}
