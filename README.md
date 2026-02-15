# cost-scaling-rs

A Rust implementation of the **CS2** minimum-cost maximum-flow algorithm, based on the cost-scaling successive approximation method by Andrew V. Goldberg.

CS2 solves the [minimum-cost flow problem](https://en.wikipedia.org/wiki/Minimum-cost_flow_problem): given a directed network with arc capacities and per-unit flow costs, find the cheapest way to route a specified amount of flow from sources to sinks.

## Usage

### As a library

```rust
use cost_scaling_rs::{McmfCs2, parser};

// Option 1: Parse a DIMACS .min file
let input = std::fs::read_to_string("problem.min").unwrap();
let problem = parser::parse(&input).unwrap();
let solver = McmfCs2::from(problem);

// Option 2: Build the problem programmatically
let mut solver = McmfCs2::new(6, 8); // 6 nodes, 8 arcs
solver.set_supply_demand_of_node(1, 10);  // source: +10
solver.set_supply_demand_of_node(6, -10); // sink: -10
solver.set_arc(1, 2, 0, 4, 1); // tail, head, lower_bound, upper_bound, cost
solver.set_arc(1, 3, 0, 8, 5);
// ... more arcs ...

// Solve
let solution = solver.min_cost(false, false).unwrap();
println!("Optimal cost: {}", solution.objective_cost);

for (tail, head, flow) in solution.flows() {
    if flow > 0 {
        println!("  {} -> {}: {}", tail, head, flow);
    }
}
```

> **Note:** When building problems programmatically, call `set_supply_demand_of_node` **before** `set_arc`. This is required because `set_arc` adjusts node excess internally for arcs with nonzero lower bounds.

### As a CLI

```bash
cargo run --release -- problem.min
```

Outputs the solution in DIMACS format:
```
s <objective_cost>
f <tail> <head> <flow>
...
```

### Input format

The solver accepts the standard [DIMACS minimum-cost flow format](http://lpsolve.sourceforge.net/5.5/DIMACS_mcf.htm):

```
c comment lines
p min <nodes> <arcs>
n <node_id> <supply>
a <tail> <head> <lower_bound> <upper_bound> <cost>
```

## Testing

The test suite validates the Rust implementation against the original C reference (included in `cs2/`):

```bash
cargo test
```

This runs:
- **Unit tests** for the DIMACS parser and GOTO problem generator
- **Integration tests** that compare objective costs and per-arc flows between Rust and C across 41 problems: static test files, procedurally generated GOTO networks of various sizes (15 to 1000 nodes), and hand-crafted edge cases (parallel arcs, lower bounds, cycles, bottlenecks, etc.)

The C binary is compiled automatically on first test run.

## Project structure

```
src/
  lib.rs      - McmfCs2 solver implementation
  parser.rs   - DIMACS .min format parser
  goto.rs     - GOTO (Grid On Torus) test problem generator
  main.rs     - CLI entry point
cs2/          - Reference C implementation (Goldberg, IG Systems)
testdata/     - Static DIMACS test inputs
tests/        - Integration tests (Rust vs. C comparison)
```

## References

- A.V. Goldberg, "An Efficient Implementation of a Scaling Minimum-Cost Flow Algorithm," *Journal of Algorithms*, 22(1), 1997.
- The original C implementation: [cs2](https://github.com/iveney/cs2)

## License

The `cs2/` directory contains the original C implementation, which is Copyright (C) 1995-2009 IG Systems, Inc. It is included for testing purposes only. See `cs2/COPYRIGHT` for terms.
