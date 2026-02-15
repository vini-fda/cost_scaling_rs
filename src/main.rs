use cost_scaling_rs::McmfCs2;
use std::{env, fs, process};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <dimacs-file>", args[0]);
        process::exit(1);
    }

    let input = fs::read_to_string(&args[1]).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", args[1]);
        process::exit(1);
    });

    let solver = McmfCs2::from_dimacs(&input).unwrap_or_else(|e| {
        eprintln!("Parse error: {e}");
        process::exit(1);
    });

    let solution = solver.min_cost(false, false).unwrap_or_else(|e| {
        eprintln!("Solver error: {e:?}");
        process::exit(1);
    });

    println!("s {:.0}", solution.objective_cost);
    for (tail, head, flow) in solution.flows() {
        println!("f {tail:7} {head:7} {flow:10}");
    }
}
