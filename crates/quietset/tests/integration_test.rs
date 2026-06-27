use quietset::{parse_jsonl, score_all, Decision, ScoreConfig};

fn load(filename: &str) -> Vec<quietset::Observation> {
    let path = format!("../../tests/fixtures/{}", filename);
    let content = std::fs::read_to_string(&path).unwrap();
    parse_jsonl(&content).unwrap()
}

#[test]
fn test_simple_fixture_decisions() {
    let obs = load("simple.jsonl");
    let reports = score_all(obs, &ScoreConfig::default());
    assert_eq!(reports.len(), 2);
    let a = reports.iter().find(|r| r.sample_id == "a").unwrap();
    let b = reports.iter().find(|r| r.sample_id == "b").unwrap();
    assert_eq!(a.decision, Decision::Keep, "sample a should be kept");
    // b is noisy, should not be keep
    assert_ne!(b.decision, Decision::Keep, "sample b should not be kept");
}

#[test]
fn test_stable_scores_are_kept() {
    let obs = load("stable_scores.jsonl");
    let reports = score_all(obs, &ScoreConfig::default());
    for r in &reports {
        assert_eq!(r.decision, Decision::Keep, "{} should be kept", r.sample_id);
    }
}

#[test]
fn test_budget_sensitive_is_not_kept() {
    let obs = load("budget_sensitive.jsonl");
    let reports = score_all(obs, &ScoreConfig::default());
    assert_eq!(reports.len(), 1);
    assert_ne!(
        reports[0].decision,
        Decision::Keep,
        "budget-sensitive sample should not be kept"
    );
}

#[test]
fn test_single_observation_is_review() {
    let obs = vec![quietset::Observation {
        sample_id: "solo".into(),
        label: Some("yes".into()),
        score: Some(0.99),
        ..Default::default()
    }];
    let reports = score_all(obs, &ScoreConfig::default());
    assert_eq!(reports[0].decision, Decision::Review);
    assert!((reports[0].stability_score - 0.5).abs() < 1e-10);
}

#[test]
fn test_missing_optional_fields() {
    let jsonl = r#"{"sample_id":"a","label":"yes"}
{"sample_id":"a","label":"yes"}
{"sample_id":"a","label":"no"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    assert_eq!(reports.len(), 1);
    assert!(reports[0].score_mean.is_none());
    assert!(reports[0].label_agreement.is_some());
}

#[test]
fn test_label_agreement() {
    let jsonl = r#"{"sample_id":"a","label":"yes"}
{"sample_id":"a","label":"yes"}
{"sample_id":"a","label":"no"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let agreement = reports[0].label_agreement.unwrap();
    assert!((agreement - 2.0 / 3.0).abs() < 1e-10);
}

#[test]
fn test_invalid_jsonl_returns_error() {
    let result = parse_jsonl(
        r#"{"sample_id":"a"}
not_valid_json"#,
    );
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("line 2"),
        "error should mention line number: {err}"
    );
}

#[test]
fn test_deterministic_output_order() {
    let jsonl = r#"{"sample_id":"z","label":"a","score":0.9}
{"sample_id":"a","label":"a","score":0.9}
{"sample_id":"m","label":"a","score":0.9}
{"sample_id":"z","label":"a","score":0.9}
{"sample_id":"a","label":"a","score":0.9}
{"sample_id":"m","label":"a","score":0.9}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    // Should preserve first-seen insertion order
    assert_eq!(reports[0].sample_id, "z");
    assert_eq!(reports[1].sample_id, "a");
    assert_eq!(reports[2].sample_id, "m");
}

#[test]
fn test_grouping_by_sample_id() {
    let jsonl = r#"{"sample_id":"a","score":0.9}
{"sample_id":"b","score":0.8}
{"sample_id":"a","score":0.85}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let a = reports.iter().find(|r| r.sample_id == "a").unwrap();
    assert_eq!(a.n_observations, 2);
}

#[test]
fn test_score_mean_std_range() {
    let obs = vec![
        quietset::Observation {
            sample_id: "a".into(),
            score: Some(1.0),
            ..Default::default()
        },
        quietset::Observation {
            sample_id: "a".into(),
            score: Some(3.0),
            ..Default::default()
        },
    ];
    let reports = score_all(obs, &ScoreConfig::default());
    let r = &reports[0];
    assert!((r.score_mean.unwrap() - 2.0).abs() < 1e-10);
    assert!((r.score_range.unwrap() - 2.0).abs() < 1e-10);
}

#[test]
fn test_keep_review_drop_thresholds() {
    use quietset::decision::{decide, Thresholds};
    let t = Thresholds::default();
    assert_eq!(decide(0.9, &t), Decision::Keep);
    assert_eq!(decide(0.6, &t), Decision::Review);
    assert_eq!(decide(0.3, &t), Decision::Drop);
    // boundaries
    assert_eq!(decide(0.85, &t), Decision::Keep);
    assert_eq!(decide(0.40, &t), Decision::Drop);
}
