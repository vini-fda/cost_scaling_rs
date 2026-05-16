//! NETGEN min-cost flow problem generator — CLI.
//!
//! Reads NETGEN parameter records from stdin and writes DIMACS instances to
//! stdout, exactly mirroring the C reference's `main()` so this binary is a
//! drop-in replacement for the `netgen` C executable.
//!
//! ## Input format
//!
//! Each record is a sequence of integers separated by whitespace (spaces,
//! tabs, or newlines):
//!
//! ```text
//! seed problem nodes sources sinks density mincost maxcost supply tsources tsinks hicost capacitated mincap maxcap
//! ```
//!
//! The CLI loops until it reads a non-positive `seed` or `problem` value, or
//! reaches EOF. This matches the C generator's behavior and lets you pipe
//! `problems.40` straight in:
//!
//! ```bash
//! cargo run --release --example gen_netgen < problems.40 > out.dimacs
//! ```

use cost_scaling_rs::netgen::{NetgenParams, generate, write_dimacs};
use std::io::{self, BufWriter, Read};

fn main() {
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("netgen: failed to read stdin: {e}");
        std::process::exit(1);
    }
    let mut tokens = input.split_ascii_whitespace();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    loop {
        let seed = match next_i64(&mut tokens) {
            Some(v) => v,
            None => return,
        };
        if seed <= 0 {
            return;
        }
        let problem = match next_i64(&mut tokens) {
            Some(v) => v,
            None => return,
        };
        if problem <= 0 {
            return;
        }
        let params = match read_params(&mut tokens) {
            Some(p) => p,
            None => {
                eprintln!("netgen: truncated parameter record");
                std::process::exit(1);
            }
        };
        let inst = match generate(seed, params) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("netgen: {e}");
                // The C reference uses error codes 1000 + |rc|; we use plain
                // non-zero exits which is enough to fail benchmark scripts.
                std::process::exit(1);
            }
        };
        if let Err(e) = write_dimacs(&inst, &mut out, problem) {
            eprintln!("netgen: write error: {e}");
            std::process::exit(1);
        }
    }
}

fn next_i64<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<i64> {
    tokens.next().and_then(|t| t.parse::<i64>().ok())
}

fn next_u64<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<u64> {
    tokens.next().and_then(|t| t.parse::<u64>().ok())
}

fn read_params<'a>(tokens: &mut impl Iterator<Item = &'a str>) -> Option<NetgenParams> {
    Some(NetgenParams {
        nodes: next_u64(tokens)?,
        sources: next_u64(tokens)?,
        sinks: next_u64(tokens)?,
        density: next_u64(tokens)?,
        min_cost: next_i64(tokens)?,
        max_cost: next_i64(tokens)?,
        supply: next_i64(tokens)?,
        t_sources: next_u64(tokens)?,
        t_sinks: next_u64(tokens)?,
        hi_cost: next_u64(tokens)?,
        capacitated: next_u64(tokens)?,
        min_cap: next_i64(tokens)?,
        max_cap: next_i64(tokens)?,
    })
}
