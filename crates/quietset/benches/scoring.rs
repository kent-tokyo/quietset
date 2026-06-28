use criterion::{Criterion, criterion_group, criterion_main};
use quietset::{ScoreConfig, parse_jsonl, score_all};

fn bench_score_simple(c: &mut Criterion) {
    let input = std::fs::read_to_string("../../tests/fixtures/simple.jsonl").unwrap();
    let obs = parse_jsonl(&input).unwrap();
    c.bench_function("score_simple_5obs", |b| {
        b.iter(|| score_all(obs.clone(), &ScoreConfig::default()))
    });
}

fn bench_score_stable(c: &mut Criterion) {
    let input = std::fs::read_to_string("../../tests/fixtures/stable_scores.jsonl").unwrap();
    let obs = parse_jsonl(&input).unwrap();
    c.bench_function("score_stable_6obs", |b| {
        b.iter(|| score_all(obs.clone(), &ScoreConfig::default()))
    });
}

criterion_group!(benches, bench_score_simple, bench_score_stable);
criterion_main!(benches);
