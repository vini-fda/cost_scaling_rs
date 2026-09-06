//! Typst/Fletcher diagrams for generated graph problems.
//!
//! Use [`super::goto::write_typst`] or [`super::netgen::write_typst`] to write a
//! standalone `.typ` file. Compile it with Typst and Fletcher 0.5.8:
//!
//! ```text
//! typst compile problem.typ light.png
//! typst compile problem.typ dark.png --input theme=dark
//! typst compile problem.typ detail-{p}.png --input view=details
//! ```
//!
//! The overview partitions GOTO arcs into four disjoint panels and uses a
//! layered layout for NETGEN. Detail pages show at most 12 arcs at a time,
//! with capacity/cost labels and an exact arc register. Every arc retains its
//! original, one-based generation index, including parallel arcs.
//!
//! Diagrams are intended for small explanatory instances. Larger inputs return
//! an error instead of silently sampling the graph or producing an unreadable
//! image. No Typst subprocess is started by the library.

use std::fmt;
use std::io::{self, Write};

use super::netgen::ProblemKind;

/// Maximum number of nodes accepted by the diagram writers.
pub const MAX_NODES: u64 = 128;
/// Maximum number of arcs accepted by the diagram writers.
pub const MAX_ARCS: u64 = 2048;

/// Errors produced while preparing or writing a diagram.
#[derive(Debug)]
pub enum DiagramError {
    /// The instance exceeds [`MAX_NODES`] or [`MAX_ARCS`].
    TooLarge {
        /// Number of nodes requested.
        nodes: u64,
        /// Number of arcs requested.
        arcs: u64,
    },
    /// A publicly constructed instance contains inconsistent graph data.
    InvalidInstance(&'static str),
    /// GOTO rejected its generation parameters.
    Goto(super::goto::GotoError),
    /// The generated GOTO DIMACS data could not be parsed.
    Parse(crate::parser::ParseError),
    /// The output writer failed.
    Io(io::Error),
}

impl fmt::Display for DiagramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { nodes, arcs } => write!(
                f,
                "diagram has {nodes} nodes and {arcs} arcs; limits are {MAX_NODES} nodes and {MAX_ARCS} arcs"
            ),
            Self::InvalidInstance(message) => write!(f, "invalid diagram instance: {message}"),
            Self::Goto(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DiagramError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Goto(error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::TooLarge { .. } | Self::InvalidInstance(_) => None,
        }
    }
}

impl From<io::Error> for DiagramError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(super) fn check_size(nodes: u64, arcs: u64) -> Result<(), DiagramError> {
    if nodes > MAX_NODES || arcs > MAX_ARCS {
        return Err(DiagramError::TooLarge { nodes, arcs });
    }
    Ok(())
}

pub(super) struct DiagramNode {
    pub position: (f64, f64),
    pub balance: i64,
}

pub(super) struct DiagramArc {
    pub from: usize,
    pub to: usize,
    pub capacity: i64,
    pub cost: i64,
    pub group: usize,
}

pub(super) struct Group {
    pub title: &'static str,
    pub description: &'static str,
}

pub(super) struct Graph {
    pub family: &'static str,
    pub kind: ProblemKind,
    pub seed: i64,
    pub nodes: Vec<DiagramNode>,
    pub arcs: Vec<DiagramArc>,
    pub groups: Vec<Group>,
}

impl Graph {
    // Parallel and antiparallel arcs get different physical lanes. Long edges
    // bend around intervening nodes. Fletcher's y axis points DOWN.
    fn bend(&self, index: usize) -> f64 {
        let arc = &self.arcs[index];
        if arc.from == arc.to {
            return 130.0;
        }
        let pair = (arc.from.min(arc.to), arc.from.max(arc.to));
        let mut count = 0;
        let mut lane = 0;
        for (i, candidate) in self.arcs.iter().enumerate() {
            if (
                candidate.from.min(candidate.to),
                candidate.from.max(candidate.to),
            ) == pair
            {
                if i < index {
                    lane += 1;
                }
                count += 1;
            }
        }
        let (ax, ay) = self.nodes[arc.from - 1].position;
        let (bx, by) = self.nodes[arc.to - 1].position;
        let obstructed = self.nodes.iter().enumerate().any(|(i, node)| {
            let (px, py) = node.position;
            i + 1 != arc.from
                && i + 1 != arc.to
                && ((px - ax) * (by - ay) - (py - ay) * (bx - ax)).abs() < 0.001
                && (px - ax) * (px - bx) + (py - ay) * (py - by) < 0.0
        });
        let direction = if arc.from < arc.to { 1.0 } else { -1.0 };
        let bend = if count == 1 {
            if obstructed { 28.0 } else { 0.0 }
        } else if obstructed {
            // Distinct lanes on the same side, all clear of intermediate nodes.
            16.0 + f64::from(lane) * (54.0 / f64::from(count - 1)).min(18.0)
        } else {
            (f64::from(lane) - f64::from(count - 1) / 2.0)
                * (140.0 / f64::from(count - 1)).min(22.0)
        };
        direction * bend
    }

    pub(super) fn write(&self, out: &mut impl Write) -> Result<(), DiagramError> {
        writeln!(
            out,
            "// Generated by cost-scaling-rs. All node and arc IDs are one-based."
        )?;
        writeln!(
            out,
            "// typst compile problem.typ problem.png --input theme=dark"
        )?;
        writeln!(
            out,
            "// Use --input view=details for labeled arc pages (output: detail-{{p}}.png)."
        )?;
        writeln!(out, "#let family = \"{}\"", self.family)?;
        let kind = match self.kind {
            ProblemKind::MinCostFlow => "Minimum-cost flow",
            ProblemKind::MaxFlow => "Maximum flow",
            ProblemKind::Assignment => "Assignment",
        };
        writeln!(out, "#let problem-kind = \"{kind}\"")?;
        writeln!(out, "#let seed = {}", self.seed)?;
        writeln!(out, "#let vertices = (")?;
        for (i, node) in self.nodes.iter().enumerate() {
            let (x, y) = node.position;
            let balance = node.balance;
            let role = if balance > 0 {
                "source"
            } else if balance < 0 {
                "sink"
            } else {
                "transit"
            };
            let badge = match self.kind {
                ProblemKind::MaxFlow if balance > 0 => "S".to_owned(),
                ProblemKind::MaxFlow if balance < 0 => "T".to_owned(),
                _ if balance > 0 => format!("+{balance}"),
                _ if balance < 0 => balance.to_string(),
                _ => String::new(),
            };
            writeln!(
                out,
                "  (id: {}, pos: ({x:.3}, {y:.3}), role: \"{role}\", badge: \"{badge}\"),",
                i + 1
            )?;
        }
        writeln!(out, ")\n#let arcs = (")?;
        for (i, arc) in self.arcs.iter().enumerate() {
            writeln!(
                out,
                "  (id: {}, tail: {}, head: {}, cap: {}, cost: {}, group: {}, bend: {:.3}),",
                i + 1,
                arc.from,
                arc.to,
                arc.capacity,
                arc.cost,
                arc.group,
                self.bend(i)
            )?;
        }
        writeln!(out, ")\n#let panels = (")?;
        for group in &self.groups {
            writeln!(
                out,
                "  (title: \"{}\", description: \"{}\"),",
                group.title, group.description
            )?;
        }
        writeln!(out, ")")?;
        out.write_all(include_bytes!("template.typ"))?;
        Ok(())
    }
}
