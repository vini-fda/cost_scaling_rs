//! Diagram export preserves the generators' actual graph data and identities.

use cost_scaling_rs::parser;
use cost_scaling_rs::problem_generators::{goto, netgen, typst::DiagramError};
use std::io;

fn goto_params(n: i64) -> goto::GotoParams {
    goto::GotoParams {
        n,
        m: n * 6,
        max_cap: 10,
        max_cost: 10,
        seed: 42,
    }
}

fn netgen_params() -> netgen::NetgenParams {
    netgen::NetgenParams {
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
    }
}

fn netgen_source(instance: &netgen::NetgenInstance) -> String {
    let mut output = Vec::new();
    netgen::write_typst(instance, &mut output).expect("diagram");
    String::from_utf8(output).expect("UTF-8 Typst source")
}

#[test]
fn diagrams_use_the_exact_sample_theme_and_fletcher_defaults() {
    let sample = include_str!("../diagrams/sample.typ");
    let instance = netgen::generate(13_502_460, netgen_params()).expect("NETGEN");
    let source = netgen_source(&instance);
    let theme_start = sample.find("#let theme =").expect("sample theme");
    let theme_end = sample.find("#let colred").expect("end of sample theme");
    assert!(source.contains(&sample[theme_start..theme_end]));
    let defaults = sample
        .split_once("#diagram(\n")
        .expect("sample diagram")
        .1
        .split_once("\n\n")
        .expect("sample defaults")
        .0;
    for setting in defaults.lines().map(str::trim) {
        assert!(
            source.lines().any(|line| line.trim() == setting),
            "missing sample setting: {setting}"
        );
    }
    assert!(
        !source.contains("font:"),
        "inherit Typst's default font, as the sample does"
    );
}

fn arc_records(source: &str) -> Vec<Vec<i64>> {
    source
        .lines()
        .filter(|line| line.contains("tail:"))
        .map(|line| {
            line.trim()
                .trim_start_matches('(')
                .trim_end_matches(",")
                .trim_end_matches(')')
                .split(',')
                .take(6)
                .map(|field| {
                    field
                        .split_once(':')
                        .expect("named field")
                        .1
                        .trim()
                        .parse()
                        .expect("integer")
                })
                .collect()
        })
        .collect()
}

#[test]
fn goto_diagrams_preserve_every_generated_arc_and_grid_coordinates() {
    // Includes exact grids, extra nodes, and the smallest supported instance.
    for n in [15, 16, 20, 27, 50] {
        let params = goto_params(n);
        let dimacs = goto::generate_to_string(&params).expect("GOTO");
        let original = parser::parse(&dimacs).expect("DIMACS");
        let mut output = Vec::new();
        goto::write_typst(&params, &mut output).expect("diagram");
        let source = String::from_utf8(output).expect("Typst");
        let records = arc_records(&source);
        assert_eq!(records.len(), original.arcs.len());
        for (i, (record, arc)) in records.iter().zip(&original.arcs).enumerate() {
            assert_eq!(
                &record[..5],
                &[i as i64 + 1, arc.from, arc.to, arc.max_cap, arc.cost]
            );
            assert!((0..=3).contains(&record[5]));
        }
        let width: i64 = original
            .comments
            .iter()
            .find_map(|c| c.strip_prefix("X="))
            .expect("grid comment")
            .split_whitespace()
            .next()
            .expect("width")
            .parse()
            .expect("integer");
        for i in 0..n {
            let expected = format!(
                "(id: {}, pos: ({:.3}, {:.3}),",
                i + 1,
                (i % width) as f64,
                (i / width) as f64
            );
            assert!(source.contains(&expected), "missing coordinate {expected}");
        }
        // The return path must remain the final grid-size - 1 arcs.
        let rows: i64 = original
            .comments
            .iter()
            .find(|c| c.starts_with("X="))
            .expect("grid")
            .split_whitespace()
            .nth(1)
            .expect("height")
            .strip_prefix("Y=")
            .expect("Y")
            .parse()
            .expect("integer");
        assert_eq!(
            records.iter().filter(|r| r[5] == 3).count(),
            (width * rows - 1) as usize
        );
        assert_eq!(source.matches("role:").count(), n as usize);
        if n == 15 {
            // Grid and return-path arcs share these endpoints and appear
            // together on detail pages, despite belonging to separate views.
            let bends: Vec<_> = source
                .lines()
                .filter(|line| line.contains("tail: 1, head: 6,"))
                .map(|line| line.split_once("bend:").expect("bend").1)
                .collect();
            assert!(bends.len() > 1);
            let unique: std::collections::BTreeSet<_> = bends.iter().collect();
            assert_eq!(
                unique.len(),
                bends.len(),
                "parallel arcs need distinct routes across views"
            );
        }
    }
}

#[test]
fn netgen_diagrams_preserve_all_three_problem_kinds() {
    let params = netgen_params();
    for (params, kind, title) in [
        (
            params,
            netgen::ProblemKind::MinCostFlow,
            "Minimum-cost flow",
        ),
        (
            netgen::NetgenParams {
                min_cost: 1,
                max_cost: 1,
                ..params
            },
            netgen::ProblemKind::MaxFlow,
            "Maximum flow",
        ),
        (
            netgen::NetgenParams {
                nodes: 8,
                sources: 4,
                sinks: 4,
                density: 12,
                supply: 4,
                ..params
            },
            netgen::ProblemKind::Assignment,
            "Assignment",
        ),
    ] {
        let instance = netgen::generate(13_502_460, params).expect("NETGEN");
        assert_eq!(instance.kind, kind);
        let source = netgen_source(&instance);
        assert_eq!(
            source,
            netgen_source(&instance),
            "layout must be deterministic"
        );
        assert!(source.contains(&format!("#let problem-kind = \"{title}\"")));
        let records = arc_records(&source);
        assert_eq!(records.len(), instance.from.len());
        for (i, record) in records.iter().enumerate() {
            assert_eq!(
                record,
                &[
                    i as i64 + 1,
                    instance.from[i] as i64,
                    instance.to[i] as i64,
                    instance.cap[i],
                    instance.cost[i],
                    0
                ]
            );
        }
        if kind == netgen::ProblemKind::MaxFlow {
            assert!(source.contains("badge: \"S\""));
            assert!(source.contains("badge: \"T\""));
            assert!(!source.contains("badge: \"+"));
        }
    }
}

#[test]
fn parallel_reverse_and_self_arcs_keep_distinct_identities() {
    let instance = netgen::NetgenInstance {
        params: netgen::NetgenParams {
            nodes: 2,
            sources: 1,
            sinks: 1,
            density: 4,
            ..netgen_params()
        },
        seed: 1,
        kind: netgen::ProblemKind::MinCostFlow,
        supply: vec![10, -10],
        from: vec![1, 1, 2, 1],
        to: vec![2, 2, 1, 1],
        cap: vec![0, i64::MAX, 10, 1],
        cost: vec![-5, i64::MIN, 9, 0],
    };
    let source = netgen_source(&instance);
    let arcs = arc_records(&source);
    assert_eq!(arcs.len(), 4);
    assert_eq!(&arcs[1][..5], &[2, 1, 2, i64::MAX, i64::MIN]);
    assert!(
        source
            .lines()
            .any(|line| line.contains("tail: 1, head: 1") && line.contains("bend: 130.0"))
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| line.contains("tail: 1, head: 2"))
            .count(),
        2
    );
}

#[test]
fn invalid_and_oversized_inputs_fail_before_output() {
    for params in [
        goto::GotoParams {
            n: i64::MAX,
            m: i64::MAX,
            ..goto_params(15)
        },
        goto_params(129),
    ] {
        let mut output = Vec::new();
        assert!(matches!(
            goto::write_typst(&params, &mut output),
            Err(DiagramError::TooLarge { .. })
        ));
        assert!(output.is_empty());
    }
    assert!(matches!(
        goto::write_typst(&goto_params(3), &mut Vec::new()),
        Err(DiagramError::Goto(_))
    ));
    let valid = netgen::generate(13_502_460, netgen_params()).expect("NETGEN");
    for mutation in 0..6 {
        let mut instance = valid.clone();
        match mutation {
            0 => {
                instance.to.pop();
            }
            1 => {
                instance.supply.pop();
            }
            2 => {
                instance.from[0] = 0;
            }
            3 => {
                instance.to[0] = instance.params.nodes + 1;
            }
            4 => {
                instance.params.sinks = u64::MAX;
            }
            _ => {
                instance.cap[0] = -1;
            }
        }
        let mut output = Vec::new();
        assert!(matches!(
            netgen::write_typst(&instance, &mut output),
            Err(DiagramError::InvalidInstance(_))
        ));
        assert!(output.is_empty());
    }
}

#[test]
fn output_errors_are_returned_by_both_writers() {
    struct BrokenWriter;
    impl io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(
        matches!(goto::write_typst(&goto_params(15), &mut BrokenWriter), Err(DiagramError::Io(e)) if e.kind() == io::ErrorKind::BrokenPipe)
    );
    let instance = netgen::generate(13_502_460, netgen_params()).expect("NETGEN");
    assert!(
        matches!(netgen::write_typst(&instance, &mut BrokenWriter), Err(DiagramError::Io(e)) if e.kind() == io::ErrorKind::BrokenPipe)
    );
}
