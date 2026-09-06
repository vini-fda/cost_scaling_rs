use std::io::Write;

use super::NetgenInstance;
use crate::problem_generators::typst::{self, DiagramArc, DiagramError, DiagramNode, Graph, Group};

/// Write a standalone Typst/Fletcher diagram of a NETGEN instance.
///
/// Sources occupy the left column, sinks the right, and transshipment nodes
/// the middle. Supports minimum-cost flow, maximum flow, and assignment;
/// labels follow the semantics of each DIMACS problem kind.
/// Compile with `--input theme=dark` for the dark theme, or
/// `--input view=details` for pages containing labeled arcs and exact values.
///
/// # Errors
/// Returns [`DiagramError`] for inconsistent public instance fields, instances
/// exceeding the diagram size limits, or output failures. Validation precedes output.
pub fn write_typst(instance: &NetgenInstance, out: &mut impl Write) -> Result<(), DiagramError> {
    let params = &instance.params;
    let count = instance.from.len();
    typst::check_size(params.nodes, count as u64)?;
    let n = params.nodes as usize;
    if n == 0 || instance.supply.len() != n {
        return Err(DiagramError::InvalidInstance(
            "node and supply counts disagree",
        ));
    }
    if instance.to.len() != count || instance.cap.len() != count || instance.cost.len() != count {
        return Err(DiagramError::InvalidInstance(
            "arc arrays have different lengths",
        ));
    }
    if params.sources == 0
        || params.sinks == 0
        || params.sources > params.nodes
        || params.sinks > params.nodes - params.sources
    {
        return Err(DiagramError::InvalidInstance("invalid source/sink counts"));
    }
    if instance
        .from
        .iter()
        .chain(&instance.to)
        .any(|&id| id == 0 || id > params.nodes)
    {
        return Err(DiagramError::InvalidInstance(
            "arc endpoint outside 1..=nodes",
        ));
    }
    if instance.cap.iter().any(|&cap| cap < 0) {
        return Err(DiagramError::InvalidInstance("negative arc capacity"));
    }

    let sources = params.sources as usize;
    let sinks = params.sinks as usize;
    let mut columns: Vec<Vec<usize>> = vec![(0..sources).collect()];
    if sources + sinks < n {
        columns.push((sources..n - sinks).collect());
    }
    columns.push((n - sinks..n).collect());
    // Barycentric sweeps reduce crossings without changing node identities.
    // Ties keep the existing order, making output deterministic.
    let mut ranks = vec![0.0; n];
    for column in &columns {
        for (row, &id) in column.iter().enumerate() {
            ranks[id] = row as f64;
        }
    }
    for _ in 0..4 {
        for column in &mut columns {
            let mut scores = vec![(0.0, 0usize); n];
            for (&from, &to) in instance.from.iter().zip(&instance.to) {
                let (a, b) = (from as usize - 1, to as usize - 1);
                scores[a].0 += ranks[b];
                scores[a].1 += 1;
                scores[b].0 += ranks[a];
                scores[b].1 += 1;
            }
            let score = |id: usize| {
                if scores[id].1 == 0 {
                    ranks[id]
                } else {
                    scores[id].0 / scores[id].1 as f64
                }
            };
            column.sort_by(|&a, &b| score(a).total_cmp(&score(b)));
            for (row, &id) in column.iter().enumerate() {
                ranks[id] = row as f64;
            }
        }
    }
    let height = columns.iter().map(Vec::len).max().unwrap_or(1);
    let mut nodes: Vec<_> = instance
        .supply
        .iter()
        .map(|&balance| DiagramNode {
            position: (0.0, 0.0),
            balance,
        })
        .collect();
    for (column, ids) in columns.iter().enumerate() {
        for (row, &id) in ids.iter().enumerate() {
            nodes[id].position = (
                column as f64 * 2.5,
                row as f64 + (height - ids.len()) as f64 / 2.0,
            );
        }
    }
    let arcs = (0..count)
        .map(|i| DiagramArc {
            from: instance.from[i] as usize,
            to: instance.to[i] as usize,
            capacity: instance.cap[i],
            cost: instance.cost[i],
            group: 0,
        })
        .collect();
    Graph {
        family: "NETGEN",
        kind: instance.kind,
        seed: instance.seed,
        nodes,
        arcs,
        groups: vec![Group {
            title: "Network topology",
            description: if sources + sinks == n {
                "Sources at left, sinks at right. Node IDs follow the original instance."
            } else {
                "Sources at left, transshipment nodes in the middle, sinks at right."
            },
        }],
    }
    .write(out)
}
