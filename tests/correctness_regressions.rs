//! Counterexamples from the correctness audit, checked independently of C.

#![allow(clippy::float_cmp)]

use cost_scaling_rs::{Cs2Error, DimacsLoadError, McmfCs2, ParseError, parser};

const LOWER: &str = "p min 3 2\nn 1 10\nn 3 -10\na 1 2 3 10 2\na 2 3 3 10 3\n";
const ONE_ARC: &str = "p min 2 1\nn 1 1\nn 2 -1\na 1 2 0 1 7\n";

#[test]
fn missing_and_extra_dimacs_arcs_are_errors() {
    for (declared, actual) in [(2, 1), (0, 1)] {
        let input = format!("p min 2 {declared}\nn 1 1\nn 2 -1\na 1 2 0 1 1\n");
        assert!(matches!(
            McmfCs2::from_dimacs(&input),
            Err(DimacsLoadError::Parse(ParseError::ArcCountMismatch { expected, actual: got }))
                if expected == declared && got == actual
        ));
    }
}

#[test]
fn incomplete_programmatic_builder_is_an_error() {
    let mut solver = McmfCs2::new(2, 2);
    solver.set_supply_demand_of_node(1, 1).expect("source");
    solver.set_supply_demand_of_node(2, -1).expect("sink");
    solver.set_arc(1, 2, 0, 1, 1).expect("first arc");
    assert!(matches!(
        solver.min_cost(),
        Err(Cs2Error::ArcCountMismatch {
            expected: 2,
            actual: 1
        })
    ));
}

#[test]
fn excess_arc_insertion_does_not_corrupt_builder() {
    let mut solver = McmfCs2::from_dimacs(ONE_ARC).expect("input");
    assert_eq!(
        solver.set_arc(1, 2, 0, 1, 2),
        Err(Cs2Error::ArcCountMismatch {
            expected: 1,
            actual: 2
        })
    );
    assert_eq!(
        solver
            .check_solution(true)
            .min_cost()
            .expect("unchanged graph")
            .objective_cost,
        7.0
    );
}

#[test]
fn raw_parsed_problems_also_validate_counts_and_dimensions() {
    let original = parser::parse(ONE_ARC).expect("parse");
    let mut bad = original.clone();
    bad.arcs_count = 2;
    assert!(matches!(
        McmfCs2::try_from(bad),
        Err(Cs2Error::ArcCountMismatch { .. })
    ));
    for nodes in [-1, i64::MAX] {
        let mut bad = original.clone();
        bad.nodes = nodes;
        assert!(matches!(
            McmfCs2::try_from(bad),
            Err(Cs2Error::InvalidProblemSize)
        ));
    }
    let mut bad = original;
    bad.arcs_count = -1;
    assert!(matches!(
        McmfCs2::try_from(bad),
        Err(Cs2Error::InvalidProblemSize)
    ));
}

#[test]
fn parser_rejects_negative_counts_and_duplicate_problem_lines() {
    for input in ["p min -1 0\n", "p min 2 -1\n", "p min 2 0\np min 3 0\n"] {
        assert!(parser::parse(input).is_err(), "{input}");
    }
}

#[test]
fn node_ids_are_one_based() {
    let mut solver = McmfCs2::new(2, 1);
    assert!(matches!(
        solver.set_supply_demand_of_node(0, 0),
        Err(Cs2Error::NodeIdOutOfBounds { .. })
    ));
    assert!(matches!(
        solver.set_arc(0, 2, 0, 1, 1),
        Err(Cs2Error::ArcOutOfBounds { .. })
    ));
    assert!(McmfCs2::from_dimacs("p min 2 1\na -1 2 0 1 1\n").is_err());
    // This also catches truncating i64 IDs on a 32-bit host.
    assert!(McmfCs2::from_dimacs("p min 2 1\na 4294967297 2 0 1 1\n").is_err());
}

#[test]
fn isolated_supply_and_demand_are_infeasible() {
    for input in [
        "p min 4 1\nn 3 1\nn 4 -1\na 1 2 0 1 1\n",
        "p min 4 1\nn 1 1\nn 4 -1\na 2 3 0 1 1\n",
        "p min 4 1\nn 2 1\nn 4 -1\na 3 4 0 1 1\n",
        "p min 2 0\nn 1 1\nn 2 -1\n",
    ] {
        for checked in [false, true] {
            let solver = McmfCs2::from_dimacs(input).expect("valid input");
            assert!(
                matches!(
                    solver.check_solution(checked).min_cost(),
                    Err(Cs2Error::Infeasible)
                ),
                "{input}"
            );
        }
    }
}

#[test]
fn zero_balance_isolated_nodes_are_preserved() {
    let input = "p min 5 1\nn 2 1\nn 3 -1\na 2 3 0 1 1\n";
    let solution = McmfCs2::from_dimacs(input)
        .expect("input")
        .check_solution(true)
        .comp_duals(true)
        .min_cost()
        .expect("feasible");
    assert_eq!(solution.objective_cost, 1.0);
    assert_eq!(solution.flows().collect::<Vec<_>>(), [(2, 3, 1)]);
    assert_eq!(
        solution.prices().map(|(id, _)| id).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
}

#[test]
fn zero_arc_graphs_have_zero_cost() {
    for n in [0, 1, 2, 5] {
        let solution = McmfCs2::new(n, 0)
            .check_solution(true)
            .comp_duals(true)
            .min_cost()
            .expect("empty graph");
        assert_eq!(solution.objective_cost, 0.0);
        assert_eq!(solution.flows().count(), 0);
        assert_eq!(solution.prices().count(), n);
    }
}

#[test]
fn scaling_overflow_is_reported_for_either_cost_sign() {
    for cost in [1_i64 << 62, -(1_i64 << 62)] {
        for checked in [false, true] {
            let input = format!("p min 2 1\nn 1 1\nn 2 -1\na 1 2 0 1 {cost}\n");
            let solver = McmfCs2::from_dimacs(&input).expect("representable input cost");
            assert!(matches!(
                solver.check_solution(checked).min_cost(),
                Err(Cs2Error::PriceOverflow)
            ));
        }
    }
}

#[test]
fn zero_capacity_costs_are_also_checked_before_scaling() {
    let mut solver = McmfCs2::new(2, 1);
    solver.set_arc(1, 2, 0, 0, 1_i64 << 62).expect("arc");
    assert!(matches!(solver.min_cost(), Err(Cs2Error::PriceOverflow)));
}

#[test]
fn minimum_signed_cost_is_rejected_without_modifying_the_builder() {
    let mut solver = McmfCs2::new(2, 1);
    assert_eq!(
        solver.set_arc(1, 2, 0, 1, i64::MIN),
        Err(Cs2Error::PriceOverflow)
    );
    solver.set_arc(1, 2, 0, 1, 1).expect("replacement");
    assert_eq!(
        solver
            .check_solution(true)
            .min_cost()
            .expect("valid")
            .objective_cost,
        0.0
    );
}

#[test]
fn lower_bounds_work_with_and_without_checks_and_duals() {
    for checked in [false, true] {
        for duals in [false, true] {
            let solution = McmfCs2::from_dimacs(LOWER)
                .expect("input")
                .check_solution(checked)
                .comp_duals(duals)
                .min_cost()
                .expect("feasible");
            assert_eq!(solution.objective_cost, 50.0);
            assert_eq!(
                solution.flows().collect::<Vec<_>>(),
                [(1, 2, 10), (2, 3, 10)]
            );
        }
    }
}

#[test]
fn replacing_supply_updates_totals_including_sign_changes() {
    let mut solver = McmfCs2::new(2, 1);
    for supply in [1, 2, -3, 0, 2] {
        solver
            .set_supply_demand_of_node(1, supply)
            .expect("replace supply");
    }
    solver.set_supply_demand_of_node(2, -2).expect("sink");
    solver.set_arc(1, 2, 0, 2, 1).expect("arc");
    assert_eq!(
        solver
            .check_solution(true)
            .min_cost()
            .expect("balanced")
            .objective_cost,
        2.0
    );
}

#[test]
fn supply_total_overflow_is_atomic() {
    let mut solver = McmfCs2::new(2, 1);
    solver
        .set_supply_demand_of_node(1, i64::MAX)
        .expect("source");
    assert_eq!(
        solver.set_supply_demand_of_node(2, 1),
        Err(Cs2Error::ExcessOverflow)
    );
    assert_eq!(
        solver.set_supply_demand_of_node(2, i64::MIN),
        Err(Cs2Error::ExcessOverflow)
    );
    solver
        .set_supply_demand_of_node(2, -i64::MAX)
        .expect("sink");
    solver.set_arc(1, 2, 0, i64::MAX, 0).expect("arc");
    let solution = solver
        .check_solution(true)
        .min_cost()
        .expect("valid large capacity");
    assert_eq!(solution.flows().collect::<Vec<_>>(), [(1, 2, i64::MAX)]);
}

#[test]
fn supplies_cannot_overwrite_lower_bound_adjustments() {
    let mut solver = McmfCs2::from_dimacs(LOWER).expect("input");
    assert_eq!(
        solver.set_supply_demand_of_node(1, 10),
        Err(Cs2Error::InvalidBuildState)
    );
    assert_eq!(
        solver
            .check_solution(true)
            .min_cost()
            .expect("unchanged")
            .objective_cost,
        50.0
    );
}

#[test]
fn self_loops_cannot_discharge_excess_to_an_unreachable_sink() {
    let input = "p min 2 1\nn 1 1\nn 2 -1\na 1 1 0 1 -4\n";
    assert!(matches!(
        McmfCs2::from_dimacs(input).expect("input").min_cost(),
        Err(Cs2Error::Infeasible)
    ));
}

#[test]
fn self_loops_choose_their_optimal_bound() {
    for cost in [-2, 0, 2] {
        let input = format!("p min 1 1\na 1 1 3 10 {cost}\n");
        let solution = McmfCs2::from_dimacs(&input)
            .expect("input")
            .check_solution(true)
            .min_cost()
            .expect("feasible circulation");
        let flow = if cost < 0 { 10 } else { 3 };
        assert_eq!(solution.flows().collect::<Vec<_>>(), [(1, 1, flow)]);
        assert_eq!(solution.objective_cost, (flow * cost) as f64);
    }
}

#[test]
fn run_cs2_cannot_reprocess_or_mutate_a_solved_arena() {
    let mut solver = McmfCs2::from_dimacs(ONE_ARC).expect("input");
    solver.run_cs2().expect("first solve");
    assert_eq!(solver.run_cs2(), Err(Cs2Error::InvalidBuildState));
    assert_eq!(
        solver.set_arc(1, 2, 0, 1, 1),
        Err(Cs2Error::InvalidBuildState)
    );
    assert_eq!(
        solver.set_supply_demand_of_node(1, 1),
        Err(Cs2Error::InvalidBuildState)
    );
    assert!(matches!(
        solver.min_cost(),
        Err(Cs2Error::InvalidBuildState)
    ));
}

#[test]
#[cfg_attr(miri, ignore)] // Miri cannot spawn the native test executable.
fn run_cs2_prints_the_actual_objective() {
    const CHILD: &str = "CS2_OBJECTIVE_OUTPUT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let mut solver = McmfCs2::from_dimacs(ONE_ARC).expect("input");
        solver.run_cs2().expect("solve");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "run_cs2_prints_the_actual_objective",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .expect("run output probe");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    let solution_lines: Vec<_> = stdout
        .lines()
        .filter(|line| line.starts_with("s "))
        .collect();
    assert_eq!(solution_lines, ["s 7"], "{stdout}");
}

/// Enumerate every flow assignment; no CS2 or C logic is used by this oracle.
fn brute_force(
    arcs: &[(usize, usize, i64, i64, i64)],
    balance: &mut [i64],
    cost: i64,
) -> Option<i64> {
    let Some((&(tail, head, low, up, price), rest)) = arcs.split_first() else {
        return balance.iter().all(|&b| b == 0).then_some(cost);
    };
    let mut best: Option<i64> = None;
    for flow in low..=up {
        balance[tail - 1] -= flow;
        balance[head - 1] += flow;
        if let Some(candidate) = brute_force(rest, balance, cost + price * flow) {
            best = Some(best.map_or(candidate, |old| old.min(candidate)));
        }
        balance[tail - 1] += flow;
        balance[head - 1] -= flow;
    }
    best
}

#[test]
fn small_graphs_match_exhaustive_flow_enumeration() {
    let mut state = 42_u64;
    let mut next = |limit: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 32) % limit
    };
    let mut feasible = 0;
    let mut infeasible = 0;
    for case in 0..500 {
        let n = 4;
        let m = next(7) as usize;
        let supply = next(5) as i64 - 2;
        let mut balances = if case % 2 == 0 {
            [0; 4]
        } else {
            [supply, 0, 0, -supply]
        };
        let mut arcs = Vec::new();
        for _ in 0..m {
            let tail = next(n as u64) as usize + 1;
            let head = next(n as u64) as usize + 1;
            let up = next(4) as i64;
            let low = next((up + 1) as u64) as i64;
            let cost = next(9) as i64 - 4;
            arcs.push((tail, head, low, up, cost));
            if case % 2 == 0 {
                // Derive balances from a witness flow, guaranteeing feasibility.
                let flow = low + next((up - low + 1) as u64) as i64;
                balances[tail - 1] += flow;
                balances[head - 1] -= flow;
            }
        }
        let mut solver = McmfCs2::new(n, m);
        for (i, &balance) in balances.iter().enumerate() {
            solver
                .set_supply_demand_of_node(i + 1, balance)
                .expect("balance");
        }
        for &(tail, head, low, up, cost) in &arcs {
            solver.set_arc(tail, head, low, up, cost).expect("arc");
        }
        let expected = brute_force(&arcs, &mut balances, 0);
        let actual = solver.check_solution(true).comp_duals(true).min_cost();
        match (expected, actual) {
            (Some(cost), Ok(solution)) => {
                feasible += 1;
                assert_eq!(
                    solution.objective_cost, cost as f64,
                    "case {case}: {arcs:?}"
                );
                for (tail, head, flow) in solution.flows() {
                    balances[tail - 1] -= flow;
                    balances[head - 1] += flow;
                }
                assert_eq!(balances, [0; 4], "original flow conservation, case {case}");
            }
            (None, Err(Cs2Error::Infeasible)) => {
                infeasible += 1;
            }
            (expected, actual) => panic!(
                "case {case}: expected {expected:?}, got {:?}, arcs {arcs:?}",
                actual.map(|s| s.objective_cost)
            ),
        }
    }
    assert!(feasible >= 250 && infeasible >= 100);
}
