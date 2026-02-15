use cost_scaling_rs::goto::{GotoParams, generate_to_string};
use cost_scaling_rs::{McmfCs2, parser};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

fn goto_problem(n: i64) -> String {
    let m = 6 * n;
    let params = GotoParams {
        n,
        m,
        max_cap: 100,
        max_cost: 100,
        seed: 42,
    };
    generate_to_string(&params).unwrap()
}

fn bench_goto(c: &mut Criterion) {
    let mut group = c.benchmark_group("goto");
    for size in [15, 30, 60, 120, 250, 500] {
        let input = goto_problem(size);
        let problem = parser::parse(&input).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(size), &problem, |b, prob| {
            b.iter(|| {
                let solver = McmfCs2::from(prob.clone());
                solver.min_cost(false, false).unwrap()
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_goto);
criterion_main!(benches);
