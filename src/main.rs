use cost_scaling_rs::{Cs2Error, McmfCs2};

fn main() -> Result<(), Cs2Error> {
    let num_nodes = 6;
    let num_arcs = 8;
    let mut solver = McmfCs2::new(num_nodes, num_arcs);

    solver.set_arc(1, 2, 0, 4, 1);
    solver.set_arc(1, 3, 0, 8, 5);
    solver.set_arc(2, 3, 0, 5, 0);
    solver.set_arc(3, 5, 0, 10, 1);
    solver.set_arc(5, 4, 0, 8, 0);
    solver.set_arc(5, 6, 0, 8, 9);
    solver.set_arc(4, 2, 0, 8, 1);
    solver.set_arc(4, 6, 0, 8, 1);
    solver.set_supply_demand_of_node(1, 10);
    solver.set_supply_demand_of_node(6, -10);

    // solver.run_cs2(true, false)?;
    let solution = solver.min_cost(true, false)?;
    println!("optimal cost = {}", solution.objective_cost);
    println!("{:?}", solution.stats());
    for (tail, head, flow) in solution.flows() {
        println!("{} -> {}: {}", tail, head, flow);
    }

    for (node, price) in solution.prices() {
        println!("p({}) = {}", node, price);
    }
    Ok(())
}
