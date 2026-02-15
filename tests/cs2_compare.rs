//! Integration tests comparing the Rust CS2 solver against the reference C implementation.
//!
//! For each DIMACS test input, both solvers run and their objective costs and per-arc flows
//! are compared. The C binary is compiled automatically if missing.

use cost_scaling_rs::goto::{self, GotoParams};
use cost_scaling_rs::{McmfCs2, parser};
use std::collections::BTreeMap;
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
    let bin = cs2_dir.join("cs2");

    BUILD_CS2.call_once(|| {
        if !bin.exists() {
            let status = Command::new("make")
                .current_dir(&cs2_dir)
                .arg("cs2.exe")
                .status()
                .expect("failed to run make for cs2");
            assert!(status.success(), "cs2 compilation failed");
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
    let problem = parser::parse(input).expect("failed to parse DIMACS input");
    let solver = McmfCs2::from(problem);
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
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
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
        if line.starts_with("s ") {
            let val: f64 = line[2..]
                .trim()
                .parse()
                .expect("failed to parse cost from s line");
            cost = Some(val);
        } else if line.starts_with("f ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert!(parts.len() >= 4, "bad flow line: {line}");
            let tail: usize = parts[1].parse().unwrap();
            let head: usize = parts[2].parse().unwrap();
            let flow: i64 = parts[3].parse().unwrap();
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
        msg.push_str(&format!(
            "  Objective cost: Rust={:.0}  C={:.0}  {}\n",
            rust.cost,
            c.cost,
            if cost_match { "OK" } else { "DIFFER" }
        ));

        // Collect all arc keys
        let mut all_keys: Vec<_> = rust.flows.keys().chain(c.flows.keys()).cloned().collect();
        all_keys.sort();
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
            msg.push_str(&format!(
                "  Flow differences ({} arcs):\n",
                flow_diffs.len()
            ));
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

// ---- Static testdata files ----

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

// ---- Generated GOTO problems (various sizes and seeds) ----

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
