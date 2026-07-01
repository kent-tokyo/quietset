use criterion::{Criterion, criterion_group, criterion_main};
use quietset::{
    DecisionScore, Observation, ScoreConfig, StreamingScorer, compute_calibration,
    compute_fleiss_kappa, compute_krippendorff_alpha, parse_jsonl, score_all,
};

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

/// `n_samples` sample_ids with `obs_per_sample` observations each, sorted by sample_id
/// (required by `StreamingScorer`). Scores/labels vary slightly per observation so the
/// stability computation does real work instead of hitting all-identical fast paths.
fn synthetic_observations(n_samples: usize, obs_per_sample: usize) -> Vec<Observation> {
    let mut obs = Vec::with_capacity(n_samples * obs_per_sample);
    for i in 0..n_samples {
        let sample_id = format!("sample_{i:05}");
        for j in 0..obs_per_sample {
            let noise = (j as f64) * 0.01;
            obs.push(Observation {
                sample_id: sample_id.clone(),
                label: Some(if j % 4 == 0 {
                    "loss".into()
                } else {
                    "win".into()
                }),
                score: Some(0.5 + (i % 10) as f64 * 0.05 - noise),
                evaluator_id: Some(format!("eval_{j}")),
                budget: Some(4.0 + j as f64),
                seed: Some(j as u64),
                ..Default::default()
            });
        }
    }
    obs
}

fn bench_score_large(c: &mut Criterion) {
    let obs = synthetic_observations(1000, 5);
    c.bench_function("score_large_1000samples_5000obs", |b| {
        b.iter(|| score_all(obs.clone(), &ScoreConfig::default()))
    });
}

fn bench_streaming_large(c: &mut Criterion) {
    let obs = synthetic_observations(1000, 5);
    c.bench_function("streaming_large_1000samples_5000obs", |b| {
        b.iter(|| {
            let mut scorer = StreamingScorer::new(ScoreConfig::default());
            let mut reports = Vec::with_capacity(1000);
            for o in obs.clone() {
                if let Some(r) = scorer.push(o) {
                    reports.push(r);
                }
            }
            if let Some(r) = scorer.flush() {
                reports.push(r);
            }
            reports
        })
    });
}

fn bench_reliability_stats(c: &mut Criterion) {
    let obs = synthetic_observations(200, 5);
    c.bench_function("reliability_stats_200subjects_5raters", |b| {
        b.iter(|| (compute_fleiss_kappa(&obs), compute_krippendorff_alpha(&obs)))
    });
}

fn bench_calibrate(c: &mut Criterion) {
    let mut obs = synthetic_observations(500, 4);
    for (i, o) in obs.iter_mut().enumerate() {
        o.gold_label = Some(if i / 4 % 4 == 0 {
            "loss".into()
        } else {
            "win".into()
        });
    }
    c.bench_function("calibrate_500samples_gold_label", |b| {
        b.iter(|| compute_calibration(&obs, &DecisionScore::Raw, 0.95, 0.90, None, 0.40))
    });
}

criterion_group!(
    benches,
    bench_score_simple,
    bench_score_stable,
    bench_score_large,
    bench_streaming_large,
    bench_reliability_stats,
    bench_calibrate,
);
criterion_main!(benches);
