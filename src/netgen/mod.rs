//! NETGEN min-cost flow problem generator
//!
//! Bit-identical Rust port of the C version of NETGEN (Schlenker, 1989), itself a
//! functional equivalent of the Fortran NETGEN of Klingman, Napier & Stutz (1974):
//!
//! > Klingman, D., A. Napier, and J. Stutz, "NETGEN: A Program for Generating
//! > Large Scale Capacitated Assignment, Transportation, and Minimum Cost Flow
//! > Network Problems", Management Science 20, 5, 814-821 (1974).
//!
//! For a given seed and parameter set, [`generate`] produces a [`NetgenInstance`]
//! whose arcs and supplies are byte-identical to the C reference when emitted
//! via [`write_dimacs`]. The original 32-bit overflow trap for `n ≥ 2^15`
//! (see the `netgen-bcjl` notes) is sidestepped by using 64-bit arithmetic
//! throughout, so all LEMON benchmark sizes are supported.
//!
//! # Quick start
//!
//! ```no_run
//! use cost_scaling_rs::netgen::{NetgenParams, generate, write_dimacs};
//! use std::io::stdout;
//!
//! let params = NetgenParams {
//!     nodes: 200, sources: 100, sinks: 100, density: 1300,
//!     min_cost: 1, max_cost: 10_000, supply: 100_000,
//!     t_sources: 0, t_sinks: 0,
//!     hi_cost: 0, capacitated: 0, min_cap: 0, max_cap: 0,
//! };
//! let inst = generate(13_502_460, params).expect("generate");
//! write_dimacs(&inst, &mut stdout(), 1).expect("write");
//! ```

mod generator;
mod index_list;
mod rng;

use std::io::{self, Write};

pub use generator::{NetgenError, NetgenInstance, NetgenParams, ProblemKind, generate};

/// Write a [`NetgenInstance`] in DIMACS format to `w`, matching the C
/// reference's `printf` order exactly.
///
/// `problem` is the problem number used in the comment header; the C version
/// reads it from stdin and we accept it as a separate argument.
pub fn write_dimacs<W: Write>(inst: &NetgenInstance, w: &mut W, problem: i64) -> io::Result<()> {
    let p = &inst.params;
    writeln!(w, "c NETGEN flow network generator (C version)")?;
    writeln!(w, "c  Problem {problem:2} input parameters")?;
    writeln!(w, "c  ---------------------------")?;
    writeln!(w, "c   Random seed:          {:10}", inst.seed)?;
    writeln!(w, "c   Number of nodes:      {:10}", p.nodes)?;
    writeln!(w, "c   Source nodes:         {:10}", p.sources)?;
    writeln!(w, "c   Sink nodes:           {:10}", p.sinks)?;
    writeln!(w, "c   Number of arcs:       {:10}", p.density)?;
    writeln!(w, "c   Minimum arc cost:     {:10}", p.min_cost)?;
    writeln!(w, "c   Maximum arc cost:     {:10}", p.max_cost)?;
    writeln!(w, "c   Total supply:         {:10}", p.supply)?;
    writeln!(w, "c   Transshipment -")?;
    writeln!(w, "c     Sources:            {:10}", p.t_sources)?;
    writeln!(w, "c     Sinks:              {:10}", p.t_sinks)?;
    writeln!(w, "c   Skeleton arcs -")?;
    writeln!(w, "c     With max cost:      {:10}%", p.hi_cost)?;
    writeln!(w, "c     Capacitated:        {:10}%", p.capacitated)?;
    writeln!(w, "c   Minimum arc capacity: {:10}", p.min_cap)?;
    writeln!(w, "c   Maximum arc capacity: {:10}", p.max_cap)?;
    let n_arcs = inst.from.len();
    match inst.kind {
        ProblemKind::Assignment => {
            writeln!(w, "c")?;
            writeln!(w, "c  *** Assignment ***")?;
            writeln!(w, "c")?;
            writeln!(w, "p asn {} {}", p.nodes, n_arcs)?;
            for (i, &b) in inst.supply.iter().enumerate() {
                if b > 0 {
                    writeln!(w, "n {}", i + 1)?;
                }
            }
            for i in 0..n_arcs {
                writeln!(w, "a {} {} {}", inst.from[i], inst.to[i], inst.cost[i])?;
            }
        }
        ProblemKind::MaxFlow => {
            writeln!(w, "c")?;
            writeln!(w, "c  *** Maximum flow ***")?;
            writeln!(w, "c")?;
            writeln!(w, "p max {} {}", p.nodes, n_arcs)?;
            for (i, &b) in inst.supply.iter().enumerate() {
                if b > 0 {
                    writeln!(w, "n {} s", i + 1)?;
                } else if b < 0 {
                    writeln!(w, "n {} t", i + 1)?;
                }
            }
            for i in 0..n_arcs {
                writeln!(w, "a {} {} {}", inst.from[i], inst.to[i], inst.cap[i])?;
            }
        }
        ProblemKind::MinCostFlow => {
            writeln!(w, "c")?;
            writeln!(w, "c  *** Minimum cost flow ***")?;
            writeln!(w, "c")?;
            writeln!(w, "p min {} {}", p.nodes, n_arcs)?;
            for (i, &b) in inst.supply.iter().enumerate() {
                if b != 0 {
                    writeln!(w, "n {} {}", i + 1, b)?;
                }
            }
            for i in 0..n_arcs {
                writeln!(
                    w,
                    "a {} {} {} {} {}",
                    inst.from[i], inst.to[i], 0, inst.cap[i], inst.cost[i],
                )?;
            }
        }
    }
    Ok(())
}
