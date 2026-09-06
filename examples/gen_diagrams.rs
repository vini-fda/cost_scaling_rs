//! Generate small GOTO and NETGEN instances and their standalone Typst diagrams.
//!
//! Usage: cargo run --example `gen_diagrams` -- [output-directory]

use cost_scaling_rs::problem_generators::{goto, netgen};
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let directory = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/diagrams"), PathBuf::from);
    fs::create_dir_all(&directory)?;
    let params = goto::GotoParams {
        n: 15,
        m: 90,
        max_cap: 10,
        max_cost: 10,
        seed: 42,
    };
    let mut writer = BufWriter::new(File::create(directory.join("goto.typ"))?);
    goto::write_typst(&params, &mut writer)?;
    writer.flush()?;
    goto::generate_to_file(&params, &directory.join("goto.min"))?;

    let params = netgen::NetgenParams {
        nodes: 9,
        sources: 3,
        sinks: 3,
        density: 14,
        min_cost: 2,
        max_cost: 9,
        supply: 30,
        t_sources: 0,
        t_sinks: 0,
        hi_cost: 0,
        capacitated: 100,
        min_cap: 4,
        max_cap: 20,
    };
    for (name, params) in [
        ("netgen", params),
        (
            "netgen-maxflow",
            netgen::NetgenParams {
                min_cost: 1,
                max_cost: 1,
                ..params
            },
        ),
        (
            "netgen-assignment",
            netgen::NetgenParams {
                nodes: 8,
                sources: 4,
                sinks: 4,
                density: 12,
                supply: 4,
                min_cap: 1,
                max_cap: 1,
                ..params
            },
        ),
    ] {
        let instance = netgen::generate(13_502_460, params)?;
        let mut writer = BufWriter::new(File::create(directory.join(format!("{name}.typ")))?);
        netgen::write_typst(&instance, &mut writer)?;
        writer.flush()?;
        let mut writer = BufWriter::new(File::create(directory.join(format!("{name}.dimacs")))?);
        netgen::write_dimacs(&instance, &mut writer, 1)?;
        writer.flush()?;
    }
    eprintln!(
        "Wrote four diagram sources and their DIMACS instances to {}",
        directory.display()
    );
    Ok(())
}
