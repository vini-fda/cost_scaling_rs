use std::io::Write;

use super::{GotoGenerator, GotoParams};
use crate::problem_generators::netgen::ProblemKind;
use crate::problem_generators::typst::{self, DiagramArc, DiagramError, DiagramNode, Graph, Group};

/// Write a standalone Typst/Fletcher diagram of the generated GOTO instance.
///
/// The overview partitions every arc into horizontal, vertical, cross-grid,
/// or return-path panels. Grid coordinates match the generator: positive x
/// points right and positive y points down. Extra nodes occupy a final row.
/// Compile with `--input theme=dark` for the dark theme, or
/// `--input view=details` for pages containing labeled arcs and exact values.
///
/// # Errors
/// Returns [`DiagramError`] for invalid GOTO parameters, instances exceeding
/// the diagram size limits, or output failures. Validation precedes output.
pub fn write_typst(params: &GotoParams, out: &mut impl Write) -> Result<(), DiagramError> {
    typst::check_size(params.n.max(0) as u64, params.m.max(0) as u64)?;
    super::validate(params).map_err(DiagramError::Goto)?;
    let mut generator = GotoGenerator::new(params);
    generator.initialize();
    let mut dimacs = String::new();
    super::generate_impl(&mut generator, &mut dimacs)
        .map_err(|error| DiagramError::Goto(super::GotoError::Fmt(error)))?;
    let parsed = crate::parser::parse(&dimacs).map_err(DiagramError::Parse)?;
    let width = generator.x as usize;
    let grid_nodes = (generator.x * generator.y) as usize;
    let mut nodes: Vec<_> = (0..params.n as usize)
        .map(|i| DiagramNode {
            position: ((i % width) as f64, (i / width) as f64),
            balance: 0,
        })
        .collect();
    for node in parsed.node_descs {
        nodes[node.id as usize - 1].balance = node.supply;
    }
    let return_start = parsed.arcs.len() - (grid_nodes - 1);
    let arcs = parsed
        .arcs
        .iter()
        .enumerate()
        .map(|(i, arc)| {
            let from = arc.from as usize;
            let to = arc.to as usize;
            let group = if i >= return_start {
                3
            } else if from > grid_nodes || to > grid_nodes {
                2
            } else if (from - 1) / width == (to - 1) / width {
                0
            } else if (from - 1) % width == (to - 1) % width {
                1
            } else {
                2
            };
            DiagramArc {
                from,
                to,
                capacity: arc.max_cap,
                cost: arc.cost,
                group,
            }
        })
        .collect();
    Graph {
        family: "GOTO",
        kind: ProblemKind::MinCostFlow,
        seed: params.seed,
        nodes,
        arcs,
        groups: vec![
            Group {
                title: "Horizontal connections",
                description: "Arcs joining nodes in the same grid row.",
            },
            Group {
                title: "Vertical connections",
                description: "Same-column arcs, including torus wraparound.",
            },
            Group {
                title: "Cross-grid connections",
                description: "Arcs between rows and columns, plus extra-node links.",
            },
            Group {
                title: "Return path",
                description: "The final high-capacity path through every grid node.",
            },
        ],
    }
    .write(out)
}
