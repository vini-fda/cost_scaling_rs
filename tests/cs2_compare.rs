//! Integration tests comparing the Rust CS2 solver against the reference C implementation.
//!
//! For each DIMACS test input, both solvers run and their objective costs and per-arc flows
//! are compared. The C binary is compiled automatically if missing.

use cost_scaling_rs::McmfCs2;
use cost_scaling_rs::goto::{self, GotoParams};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

static BUILD_CS2: Once = Once::new();

/// Returns the path to the project root.
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Returns the path to the C cs2 binary, building it if necessary.
fn cs2_binary() -> PathBuf {
    let cs2_dir = project_root().join("cs2");
    // The C makefile emits `cs2.exe` on Windows and `cs2` everywhere else;
    // `EXE_SUFFIX` gives us ".exe" or "" accordingly.
    let bin = cs2_dir.join(format!("cs2{}", std::env::consts::EXE_SUFFIX));

    BUILD_CS2.call_once(|| {
        if !bin.exists() {
            let output = Command::new("make")
                .current_dir(&cs2_dir)
                .arg("release")
                .output()
                .expect("failed to run make for cs2");
            assert!(
                output.status.success(),
                "cs2 compilation failed (exit {}):\n--- stdout ---\n{}\n--- stderr ---\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    });

    assert!(bin.exists(), "cs2 binary not found at {}", bin.display());
    bin
}

/// Parsed output from a solver: objective cost and a map of (tail, head) -> flow.
#[derive(Debug)]
struct SolverOutput {
    cost: f64,
    flows: BTreeMap<(usize, usize), i64>,
}

/// Run the Rust solver on a DIMACS input string.
fn run_rust(input: &str) -> SolverOutput {
    let solver = McmfCs2::from_dimacs(input).expect("failed to parse DIMACS input");
    let solution = solver.min_cost(false, false).expect("Rust solver failed");

    let mut flows = BTreeMap::new();
    for (tail, head, flow) in solution.flows() {
        if flow != 0 {
            flows.insert((tail, head), flow);
        }
    }

    SolverOutput {
        cost: solution.objective_cost,
        flows,
    }
}

/// Run the C cs2 binary on a DIMACS input string (fed via stdin).
fn run_c(input: &str) -> SolverOutput {
    let bin = cs2_binary();
    let output = Command::new(&bin)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .expect("child stdin should be piped")
                .write_all(input.as_bytes())
                .expect("failed to write input to cs2 stdin");
            child.wait_with_output()
        })
        .expect("failed to run cs2 binary");

    assert!(
        output.status.success(),
        "cs2 exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("cs2 output not utf8");

    let mut cost = None;
    let mut flows = BTreeMap::new();

    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("s ") {
            let val: f64 = rest
                .trim()
                .parse()
                .expect("failed to parse cost from s line");
            cost = Some(val);
        } else if line.starts_with("f ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert!(parts.len() >= 4, "bad flow line: {line}");
            let tail: usize = parts[1].parse().expect("failed to parse tail node id");
            let head: usize = parts[2].parse().expect("failed to parse head node id");
            let flow: i64 = parts[3].parse().expect("failed to parse flow value");
            if flow != 0 {
                flows.insert((tail, head), flow);
            }
        }
    }

    SolverOutput {
        cost: cost.expect("no 's' line in cs2 output"),
        flows,
    }
}

/// Compare Rust and C solver outputs, panicking with a detailed diff on mismatch.
fn compare(name: &str, input: &str) {
    let rust = run_rust(input);
    let c = run_c(input);

    let cost_match = (rust.cost - c.cost).abs() < 0.5;

    if !cost_match || rust.flows != c.flows {
        let mut msg = format!("MISMATCH in {name}\n");
        writeln!(
            msg,
            "  Objective cost: Rust={:.0}  C={:.0}  {}",
            rust.cost,
            c.cost,
            if cost_match { "OK" } else { "DIFFER" }
        )
        .expect("write to String");

        // Collect all arc keys
        let mut all_keys: Vec<_> = rust.flows.keys().chain(c.flows.keys()).copied().collect();
        all_keys.sort_unstable();
        all_keys.dedup();

        let mut flow_diffs = Vec::new();
        for key in &all_keys {
            let rf = rust.flows.get(key).copied().unwrap_or(0);
            let cf = c.flows.get(key).copied().unwrap_or(0);
            if rf != cf {
                flow_diffs.push(format!("    arc {key:?}: Rust={rf} C={cf}"));
            }
        }
        if !flow_diffs.is_empty() {
            writeln!(msg, "  Flow differences ({} arcs):", flow_diffs.len())
                .expect("write to String");
            for d in &flow_diffs {
                msg.push_str(d);
                msg.push('\n');
            }
        }

        panic!("{msg}");
    }
}

/// Helper to run comparison on a testdata file.
fn compare_file(filename: &str) {
    let path = project_root().join("testdata").join(filename);
    let input = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    compare(filename, &input);
}

/// Helper to run comparison on a generated GOTO problem.
fn compare_goto(params: &GotoParams) {
    let input = goto::generate_to_string(params).expect("GOTO generation failed");
    let name = format!(
        "goto_n{}_m{}_cap{}_cost{}_seed{}",
        params.n, params.m, params.max_cap, params.max_cost, params.seed
    );
    compare(&name, &input);
}

// ==========================================================================
// Static testdata files
// ==========================================================================

#[test]
fn testdata_case1() {
    compare_file("case1.txt");
}

#[test]
fn testdata_case2() {
    compare_file("case2.txt");
}

#[test]
fn testdata_case3() {
    compare_file("case3.txt");
}

#[test]
fn testdata_sample() {
    compare_file("sample.inp");
}

// ==========================================================================
// Generated GOTO problems — various sizes and seeds
// ==========================================================================

#[test]
fn goto_small_n15() {
    compare_goto(&GotoParams {
        n: 15,
        m: 90,
        max_cap: 10,
        max_cost: 10,
        seed: 1,
    });
}

#[test]
fn goto_small_n20() {
    compare_goto(&GotoParams {
        n: 20,
        m: 120,
        max_cap: 16,
        max_cost: 16,
        seed: 2,
    });
}

#[test]
fn goto_medium_n50() {
    compare_goto(&GotoParams {
        n: 50,
        m: 300,
        max_cap: 100,
        max_cost: 100,
        seed: 3,
    });
}

#[test]
fn goto_medium_n100() {
    compare_goto(&GotoParams {
        n: 100,
        m: 600,
        max_cap: 200,
        max_cost: 200,
        seed: 4,
    });
}

#[test]
fn goto_medium_n200() {
    compare_goto(&GotoParams {
        n: 200,
        m: 1200,
        max_cap: 500,
        max_cost: 500,
        seed: 5,
    });
}

#[test]
fn goto_large_n500() {
    compare_goto(&GotoParams {
        n: 500,
        m: 3000,
        max_cap: 1000,
        max_cost: 1000,
        seed: 6,
    });
}

#[test]
fn goto_large_n1000() {
    compare_goto(&GotoParams {
        n: 1000,
        m: 6000,
        max_cap: 2000,
        max_cost: 2000,
        seed: 10,
    });
}

#[test]
fn goto_seed_sweep_1() {
    compare_goto(&GotoParams {
        n: 30,
        m: 180,
        max_cap: 50,
        max_cost: 50,
        seed: 42,
    });
}

#[test]
fn goto_seed_sweep_2() {
    compare_goto(&GotoParams {
        n: 30,
        m: 180,
        max_cap: 50,
        max_cost: 50,
        seed: 99,
    });
}

#[test]
fn goto_seed_sweep_3() {
    compare_goto(&GotoParams {
        n: 30,
        m: 180,
        max_cap: 50,
        max_cost: 50,
        seed: 137,
    });
}

#[test]
fn goto_seed_sweep_4() {
    compare_goto(&GotoParams {
        n: 30,
        m: 180,
        max_cap: 50,
        max_cost: 50,
        seed: 256,
    });
}

#[test]
fn goto_high_capacity() {
    compare_goto(&GotoParams {
        n: 50,
        m: 300,
        max_cap: 10000,
        max_cost: 10,
        seed: 7,
    });
}

#[test]
fn goto_high_cost() {
    compare_goto(&GotoParams {
        n: 50,
        m: 300,
        max_cap: 10,
        max_cost: 10000,
        seed: 8,
    });
}

// ==========================================================================
// Hand-crafted DIMACS problems — edge cases and specific topologies
// ==========================================================================

/// Minimal 2-node, 1-arc network.
#[test]
fn hand_minimal_two_nodes() {
    let input = "\
p min 2 1
n 1 5
n 2 -5
a 1 2 0 10 3
";
    compare("minimal_two_nodes", input);
}

/// Single path: 1 -> 2 -> 3 -> 4, no choice.
#[test]
fn hand_single_path() {
    let input = "\
p min 4 3
n 1 10
n 4 -10
a 1 2 0 10 1
a 2 3 0 10 2
a 3 4 0 10 3
";
    compare("single_path", input);
}

/// Two parallel paths with different costs: the solver should pick the cheaper one.
#[test]
fn hand_two_parallel_paths() {
    let input = "\
p min 4 4
n 1 10
n 4 -10
a 1 2 0 10 1
a 2 4 0 10 1
a 1 3 0 10 5
a 3 4 0 10 5
";
    compare("two_parallel_paths", input);
}

/// Diamond graph: flow must split across two paths to respect capacity.
#[test]
fn hand_diamond_split() {
    let input = "\
p min 4 4
n 1 10
n 4 -10
a 1 2 0 6 1
a 1 3 0 6 2
a 2 4 0 6 1
a 3 4 0 6 2
";
    compare("diamond_split", input);
}

/// Zero-cost network: optimal cost should be 0.
#[test]
fn hand_zero_cost() {
    let input = "\
p min 3 2
n 1 7
n 3 -7
a 1 2 0 10 0
a 2 3 0 10 0
";
    compare("zero_cost", input);
}

/// Multiple sources and sinks with balanced supply/demand.
#[test]
fn hand_multi_source_sink() {
    let input = "\
p min 6 7
n 1 10
n 2 5
n 5 -8
n 6 -7
a 1 3 0 10 2
a 2 3 0 5 3
a 3 4 0 15 1
a 4 5 0 8 2
a 4 6 0 7 4
a 1 4 0 5 5
a 2 4 0 5 1
";
    compare("multi_source_sink", input);
}

/// Unit capacities on all arcs.
#[test]
fn hand_unit_capacities() {
    let input = "\
p min 5 6
n 1 3
n 5 -3
a 1 2 0 1 2
a 1 3 0 1 5
a 1 4 0 1 3
a 2 5 0 1 1
a 3 5 0 1 1
a 4 5 0 1 1
";
    compare("unit_capacities", input);
}

/// Longer chain: forces flow through many hops.
#[test]
fn hand_long_chain() {
    let input = "\
p min 8 7
n 1 20
n 8 -20
a 1 2 0 20 1
a 2 3 0 20 1
a 3 4 0 20 1
a 4 5 0 20 1
a 5 6 0 20 1
a 6 7 0 20 1
a 7 8 0 20 1
";
    compare("long_chain", input);
}

/// Graph with a cycle: solver must handle cycles correctly.
#[test]
fn hand_cycle() {
    let input = "\
p min 4 5
n 1 10
n 4 -10
a 1 2 0 10 1
a 2 3 0 10 1
a 3 4 0 10 1
a 3 2 0 5 1
a 2 4 0 10 5
";
    compare("cycle", input);
}

/// Large supply with tight bottleneck.
#[test]
fn hand_bottleneck() {
    let input = "\
p min 4 4
n 1 100
n 4 -100
a 1 2 0 100 1
a 1 3 0 100 1
a 2 4 0 50 1
a 3 4 0 50 1
";
    compare("bottleneck", input);
}

/// Nonzero lower bounds on arcs.
#[test]
fn hand_lower_bounds() {
    let input = "\
p min 3 2
n 1 10
n 3 -10
a 1 2 3 10 2
a 2 3 3 10 3
";
    compare("lower_bounds", input);
}

/// Star topology: one source, many sinks.
#[test]
fn hand_star_source() {
    let input = "\
p min 6 5
n 1 20
n 2 -4
n 3 -4
n 4 -4
n 5 -4
n 6 -4
a 1 2 0 10 1
a 1 3 0 10 2
a 1 4 0 10 3
a 1 5 0 10 4
a 1 6 0 10 5
";
    compare("star_source", input);
}

/// Reverse star: many sources, one sink.
#[test]
fn hand_star_sink() {
    let input = "\
p min 6 5
n 1 4
n 2 4
n 3 4
n 4 4
n 5 4
n 6 -20
a 1 6 0 10 1
a 2 6 0 10 2
a 3 6 0 10 3
a 4 6 0 10 4
a 5 6 0 10 5
";
    compare("star_sink", input);
}

/// Duplicate arcs between the same pair of nodes.
#[test]
fn hand_parallel_arcs() {
    let input = "\
p min 2 3
n 1 10
n 2 -10
a 1 2 0 5 1
a 1 2 0 5 3
a 1 2 0 5 10
";
    compare("parallel_arcs", input);
}

/// Balanced graph with zero total supply (transshipment only via lower bounds).
#[test]
fn hand_zero_supply() {
    let input = "\
p min 3 3
n 1 0
n 2 0
n 3 0
a 1 2 5 10 1
a 2 3 5 10 1
a 3 1 5 10 1
";
    compare("zero_supply", input);
}

/// Asymmetric costs: one direction cheap, reverse expensive.
#[test]
fn hand_asymmetric_costs() {
    let input = "\
p min 3 4
n 1 10
n 3 -10
a 1 2 0 10 1
a 2 3 0 10 1
a 2 1 0 5 100
a 3 2 0 5 100
";
    compare("asymmetric_costs", input);
}

/// Complete graph on 5 nodes (K5) — many routing options.
#[test]
fn hand_complete_k5() {
    let input = "\
p min 5 20
n 1 15
n 5 -15
a 1 2 0 5 2
a 1 3 0 5 4
a 1 4 0 5 6
a 1 5 0 5 8
a 2 1 0 5 3
a 2 3 0 5 1
a 2 4 0 5 5
a 2 5 0 5 7
a 3 1 0 5 5
a 3 2 0 5 2
a 3 4 0 5 3
a 3 5 0 5 1
a 4 1 0 5 7
a 4 2 0 5 4
a 4 3 0 5 6
a 4 5 0 5 2
a 5 1 0 5 9
a 5 2 0 5 6
a 5 3 0 5 3
a 5 4 0 5 1
";
    compare("complete_k5", input);
}

/// Large capacity values.
#[test]
fn hand_large_capacities() {
    let input = "\
p min 3 2
n 1 1000000
n 3 -1000000
a 1 2 0 1000000 1
a 2 3 0 1000000 1
";
    compare("large_capacities", input);
}

/// Large cost values.
#[test]
fn hand_large_costs() {
    let input = "\
p min 3 2
n 1 5
n 3 -5
a 1 2 0 10 100000
a 2 3 0 10 100000
";
    compare("large_costs", input);
}

// ==========================================================================
// Rust-only edge case tests (no C comparison needed)
// ==========================================================================

/// Infeasible: demand exceeds available capacity.
#[test]
fn edge_infeasible_insufficient_capacity() {
    let mut solver = McmfCs2::new(3, 2);
    solver.set_arc(1, 2, 0, 5, 1).expect("set_arc");
    solver.set_arc(2, 3, 0, 5, 1).expect("set_arc");
    solver.set_supply_demand_of_node(1, 10).expect("set_supply");
    solver
        .set_supply_demand_of_node(3, -10)
        .expect("set_supply");
    assert!(solver.min_cost(false, false).is_err());
}

/// Infeasible: sink unreachable from source.
#[test]
fn edge_infeasible_disconnected() {
    let mut solver = McmfCs2::new(4, 2);
    solver.set_arc(1, 2, 0, 10, 1).expect("set_arc");
    solver.set_arc(3, 4, 0, 10, 1).expect("set_arc");
    solver.set_supply_demand_of_node(1, 5).expect("set_supply");
    solver.set_supply_demand_of_node(4, -5).expect("set_supply");
    assert!(solver.min_cost(false, false).is_err());
}

/// Feasible with `check_solution` enabled: verifies internal consistency.
#[test]
fn edge_check_solution_passes() {
    let input = "\
p min 4 4
n 1 10
n 4 -10
a 1 2 0 10 1
a 2 4 0 10 1
a 1 3 0 10 5
a 3 4 0 10 5
";
    let solver = McmfCs2::from_dimacs(input).expect("failed to parse DIMACS input");
    let solution = solver
        .min_cost(true, true)
        .expect("should be feasible and CS-optimal");
    assert!((solution.objective_cost - 20.0).abs() < 0.5);
}

/// Supply = demand = 1 (minimal flow).
#[test]
fn edge_unit_flow() {
    let input = "\
p min 2 1
n 1 1
n 2 -1
a 1 2 0 1 7
";
    compare("unit_flow", input);
}

/// Self-loop arc (tail == head).
#[test]
fn edge_self_loop() {
    let input = "\
p min 2 2
n 1 5
n 2 -5
a 1 2 0 10 1
a 1 1 0 10 0
";
    compare("self_loop", input);
}
