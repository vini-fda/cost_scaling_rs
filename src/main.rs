use std::io::{BufWriter, Write};

use cost_scaling_rs::McmfCs2;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Usage: cost-scaling-rs <dimacs-file>")?;

    let solver = McmfCs2::from_dimacs_file(&path)?;
    let solution = solver
        .min_cost(false, false)
        .map_err(|e| format!("Solver error: {e:?}"))?;

    let mut out = BufWriter::new(std::io::stdout().lock());
    writeln!(out, "s {:.0}", solution.objective_cost)?;
    for (tail, head, flow) in solution.flows() {
        writeln!(out, "f {tail:7} {head:7} {flow:10}")?;
    }

    Ok(())
}
