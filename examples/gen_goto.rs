//! Generates GOTO (Grid On Torus) DIMACS problem files for benchmarking.
//!
//! Usage: cargo run --release --example gen_goto [OUTPUT_DIR]
//!
//! Generates problems at several sizes into OUTPUT_DIR (default: target/benchdata/).
//! Each file is named `goto_{nodes}n_{arcs}a.min`.

use cost_scaling_rs::goto::{GotoParams, generate_to_string};
use std::path::PathBuf;
use std::{env, fs};

/// (nodes, arcs_per_node_factor, max_cap, max_cost, seed)
const PROBLEMS: &[(i64, i64, i64, i64, i64)] = &[
    (500, 6, 10_000, 10_000, 42),
    (2_000, 6, 10_000, 10_000, 42),
    (5_000, 6, 10_000, 10_000, 42),
    (10_000, 6, 10_000, 10_000, 42),
];

fn main() {
    let out_dir: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/benchdata"));

    fs::create_dir_all(&out_dir).expect("failed to create output directory");

    for &(n, factor, max_cap, max_cost, seed) in PROBLEMS {
        let m = factor * n;
        let params = GotoParams {
            n,
            m,
            max_cap,
            max_cost,
            seed,
        };
        let content = generate_to_string(&params).expect("GOTO generation failed");
        let filename = format!("goto_{n}n_{m}a.min");
        let path = out_dir.join(&filename);
        fs::write(&path, content).expect("failed to write file");
        eprintln!("  {filename} ({n} nodes, {m} arcs)");
    }

    eprintln!(
        "Generated {} problems in {}",
        PROBLEMS.len(),
        out_dir.display()
    );
}
