//! Bit-identical parity test against the C reference NETGEN binary.
//!
//! For each fixture under `testdata/netgen/problem_*.min`, run our Rust
//! generator with the same seed/parameters and assert byte-for-byte equality
//! with the C reference output. Fixtures were captured from a freshly built
//! C binary at <http://archive.dimacs.rutgers.edu/pub/netflow/generators/network/netgen/>.

use std::path::PathBuf;

use cost_scaling_rs::problem_generators::netgen::{NetgenParams, generate, write_dimacs};

const SEED: i64 = 13_502_460;

#[derive(Debug, Clone, Copy)]
struct Fixture {
    problem: i64,
    params: NetgenParams,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        problem: 1,
        params: NetgenParams {
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
        },
    },
    Fixture {
        problem: 5,
        params: NetgenParams {
            nodes: 200,
            sources: 100,
            sinks: 100,
            density: 2900,
            min_cost: 1,
            max_cost: 10_000,
            supply: 100_000,
            t_sources: 0,
            t_sinks: 0,
            hi_cost: 0,
            capacitated: 0,
            min_cap: 0,
            max_cap: 0,
        },
    },
    Fixture {
        problem: 16,
        params: NetgenParams {
            nodes: 400,
            sources: 8,
            sinks: 60,
            density: 1306,
            min_cost: 1,
            max_cost: 10_000,
            supply: 400_000,
            t_sources: 0,
            t_sinks: 0,
            hi_cost: 30,
            capacitated: 20,
            min_cap: 16_000,
            max_cap: 30_000,
        },
    },
    Fixture {
        problem: 20,
        params: NetgenParams {
            nodes: 400,
            sources: 8,
            sinks: 60,
            density: 1416,
            min_cost: 1,
            max_cost: 10_000,
            supply: 400_000,
            t_sources: 5,
            t_sinks: 50,
            hi_cost: 30,
            capacitated: 40,
            min_cap: 16_000,
            max_cap: 30_000,
        },
    },
    Fixture {
        problem: 28,
        params: NetgenParams {
            nodes: 1000,
            sources: 50,
            sinks: 50,
            density: 2900,
            min_cost: 1,
            max_cost: 10_000,
            supply: 1_000_000,
            t_sources: 0,
            t_sinks: 0,
            hi_cost: 0,
            capacitated: 0,
            min_cap: 0,
            max_cap: 0,
        },
    },
    Fixture {
        problem: 36,
        params: NetgenParams {
            nodes: 8000,
            sources: 200,
            sinks: 1000,
            density: 15_000,
            min_cost: 1,
            max_cost: 10_000,
            supply: 4_000_000,
            t_sources: 100,
            t_sinks: 300,
            hi_cost: 0,
            capacitated: 0,
            min_cap: 30,
            max_cap: 30,
        },
    },
];

fn fixture_path(problem: i64) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("testdata/netgen");
    p.push(format!("problem_{problem}.min"));
    p
}

fn run_fixture(fx: &Fixture) {
    let path = fixture_path(fx.problem);
    let expected = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => panic!("missing fixture {}: {e}", path.display()),
    };
    let inst = generate(SEED, fx.params).expect("generate");
    let mut actual: Vec<u8> = Vec::with_capacity(expected.len());
    write_dimacs(&inst, &mut actual, fx.problem).expect("write");
    let actual = String::from_utf8(actual).expect("utf8");
    if expected != actual {
        let e_lines: Vec<&str> = expected.lines().collect();
        let a_lines: Vec<&str> = actual.lines().collect();
        let mut first_diff = None;
        for (i, (e, a)) in e_lines.iter().zip(a_lines.iter()).enumerate() {
            if e != a {
                first_diff = Some(i);
                break;
            }
        }
        let i = first_diff.unwrap_or(e_lines.len().min(a_lines.len()));
        panic!(
            "problem {} diverges at line {} (Rust has {} lines, C has {} lines).\n\
             Rust: {:?}\nC:    {:?}",
            fx.problem,
            i + 1,
            a_lines.len(),
            e_lines.len(),
            a_lines.get(i),
            e_lines.get(i),
        );
    }
}

#[test]
fn problem_1_byte_identical() {
    run_fixture(&FIXTURES[0]);
}

#[test]
fn problem_5_byte_identical() {
    run_fixture(&FIXTURES[1]);
}

#[test]
fn problem_16_byte_identical() {
    run_fixture(&FIXTURES[2]);
}

#[test]
fn problem_20_byte_identical() {
    run_fixture(&FIXTURES[3]);
}

#[test]
fn problem_28_byte_identical() {
    run_fixture(&FIXTURES[4]);
}

#[test]
fn problem_36_byte_identical() {
    run_fixture(&FIXTURES[5]);
}
