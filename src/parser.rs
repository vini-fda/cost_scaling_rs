//! Parser for DIMACS minimum-cost flow (`.min`) files.
//!
//! Link: [DIMACS graph format](http://dimacs.rutgers.edu/archive/Challenges/).
//!
//! To give a brief description of the format, a DIMACS graph file is a pure text file in which
//! every line begins with either the letter `c`, `p`, `n`, or `a` to specify
//! what type of information it defines.
//!
//! In the case of a minimum-cost flow problem, the lines are formatted as follows:
//!
//! * `c` indicates a comment line. The output file begins with a header made up of comment lines describing the parameters used to generate the problem.
//! * `p` indicates the problem definition. This follows the header and has the format `p min NODES DENSITY`, where:
//!   * `NODES` is the total number of nodes.
//!   * `DENSITY` is the total number of arcs.
//! * `n` indicates a node definition. The node definitions follow the problem definition, and have the format `n ID SUPPLY`, where:
//!   * `ID` is a unique numerical index given to all nodes (starting at 1).
//!   * `SUPPLY` is the supply value of the node (positive for sources, negative for sinks). In order to save space, only nodes with nonzero supply values are included.
//! * `a` indicates an arc definition. The arc definitions follow the node definitions, and have the format `a FROM TO MINCAP MAXCAP COST`, where:
//!   * `FROM` and `TO` are the node indices of the arc's origin and destination, respectively.
//!   * `MINCAP` and `MAXCAP` are the arc's lower and upper capacity bounds, respectively.
//!   * `COST` is the arc's unit flow cost.
//!
//! The output file for a maximum-flow problem follows the same format with the following exceptions:
//!
//! * The objective is `max` instead of `min`.
//! * Source and sink nodes are given a `SUPPLY` value of `s` or `t`, respectively, rather than a specific number.
//! * Arc definitions omit the cost and lower capacity bound, now having the format `a FROM TO MAXCAP`.

use std::fmt;

/// A parsed DIMACS minimum-cost flow problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimacsMin {
    /// Comment lines (without the leading `c `).
    pub comments: Vec<String>,
    /// Total number of nodes.
    pub nodes: i64,
    /// Total number of arcs (density).
    pub arcs_count: i64,
    /// Node descriptors (only nodes with nonzero supply).
    pub node_descs: Vec<NodeDesc>,
    /// Arc definitions.
    pub arcs: Vec<Arc>,
}

/// A node with nonzero supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeDesc {
    /// 1-based node index.
    pub id: i64,
    /// Supply value (positive = source, negative = sink).
    pub supply: i64,
}

/// A directed arc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arc {
    /// Origin node index.
    pub from: i64,
    /// Destination node index.
    pub to: i64,
    /// Lower capacity bound.
    pub min_cap: i64,
    /// Upper capacity bound.
    pub max_cap: i64,
    /// Unit flow cost.
    pub cost: i64,
}

/// Errors that can occur while parsing a DIMACS file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A required problem line (`p min ...`) was not found.
    MissingProblemLine,
    /// A line could not be parsed.
    InvalidLine {
        /// 1-based line number where the error occurred.
        line_num: usize,
        /// Description of what went wrong.
        message: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingProblemLine => write!(f, "missing problem line (p min ...)"),
            ParseError::InvalidLine { line_num, message } => {
                write!(f, "line {line_num}: {message}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses a DIMACS minimum-cost flow problem from a string.
pub fn parse(input: &str) -> Result<DimacsMin, ParseError> {
    let mut comments = Vec::new();
    let mut problem: Option<(i64, i64)> = None;
    let mut node_descs = Vec::new();
    let mut arcs = Vec::new();

    for (i, line) in input.lines().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let Some(kind) = parts.next() else {
            continue; // unreachable after the is_empty check above, but keeps us total
        };

        match kind {
            "c" => {
                // Everything after the leading "c" is the comment text. The first whitespace-split
                // token being "c" guarantees `trimmed` starts with "c".
                let text = trimmed.strip_prefix("c").unwrap_or(trimmed).trim_start();
                comments.push(text.to_string());
            }
            "p" => {
                let err = |msg: &str| ParseError::InvalidLine {
                    line_num,
                    message: msg.to_string(),
                };
                let format = parts.next().ok_or_else(|| err("expected 'min'"))?;
                if format != "min" {
                    return Err(err(&format!("expected 'min', got '{format}'")));
                }
                let n = parse_i64(parts.next(), line_num, "nodes")?;
                let m = parse_i64(parts.next(), line_num, "arcs")?;
                problem = Some((n, m));
            }
            "n" => {
                let id = parse_i64(parts.next(), line_num, "node id")?;
                let supply = parse_i64(parts.next(), line_num, "supply")?;
                node_descs.push(NodeDesc { id, supply });
            }
            "a" => {
                let from = parse_i64(parts.next(), line_num, "from")?;
                let to = parse_i64(parts.next(), line_num, "to")?;
                let min_cap = parse_i64(parts.next(), line_num, "min_cap")?;
                let max_cap = parse_i64(parts.next(), line_num, "max_cap")?;
                let cost = parse_i64(parts.next(), line_num, "cost")?;
                arcs.push(Arc {
                    from,
                    to,
                    min_cap,
                    max_cap,
                    cost,
                });
            }
            _ => {
                return Err(ParseError::InvalidLine {
                    line_num,
                    message: format!("unknown line type '{kind}'"),
                });
            }
        }
    }

    let (nodes, arcs_count) = problem.ok_or(ParseError::MissingProblemLine)?;

    Ok(DimacsMin {
        comments,
        nodes,
        arcs_count,
        node_descs,
        arcs,
    })
}

fn parse_i64(token: Option<&str>, line_num: usize, field: &str) -> Result<i64, ParseError> {
    let s = token.ok_or_else(|| ParseError::InvalidLine {
        line_num,
        message: format!("missing {field}"),
    })?;
    s.parse().map_err(|_| ParseError::InvalidLine {
        line_num,
        message: format!("invalid integer for {field}: '{s}'"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_case(s: &str) -> DimacsMin {
        parse(s).expect("parse should succeed")
    }

    #[test]
    fn case1() {
        let p = parse_case(include_str!("../testdata/case1.txt"));
        assert_eq!(p.nodes, 15);
        assert_eq!(p.arcs_count, 90);
        assert_eq!(p.node_descs.len(), 2);
        assert_eq!(p.node_descs[0], NodeDesc { id: 1, supply: 53 });
        assert_eq!(
            p.node_descs[1],
            NodeDesc {
                id: 15,
                supply: -53
            }
        );
        assert_eq!(p.arcs.len(), 90);
        assert_eq!(p.comments.len(), 2);
    }

    #[test]
    fn case2() {
        let p = parse_case(include_str!("../testdata/case2.txt"));
        assert_eq!(p.nodes, 20);
        assert_eq!(p.arcs_count, 120);
        assert_eq!(p.arcs.len(), 120);
        assert_eq!(
            p.arcs[0],
            Arc {
                from: 1,
                to: 2,
                min_cap: 0,
                max_cap: 5,
                cost: 11
            }
        );
    }

    #[test]
    fn case3() {
        let p = parse_case(include_str!("../testdata/case3.txt"));
        assert_eq!(p.nodes, 50);
        assert_eq!(p.arcs_count, 300);
        assert_eq!(p.arcs.len(), 300);
    }

    #[test]
    fn missing_problem_line() {
        let r = parse("c just a comment\nn 1 10\n");
        assert_eq!(r, Err(ParseError::MissingProblemLine));
    }

    #[test]
    fn invalid_line_type() {
        let r = parse("p min 2 1\nx garbage\n");
        assert!(matches!(
            r,
            Err(ParseError::InvalidLine { line_num: 2, .. })
        ));
    }

    #[test]
    fn empty_input() {
        assert_eq!(parse(""), Err(ParseError::MissingProblemLine));
    }
}
