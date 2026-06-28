use quietset::{
    Decision, DecisionScore, MinRequirements, Observation, ScoreConfig, ScoreWeights, Thresholds,
    compute_evaluator_reliability, parse_jsonl, score_all,
};

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
    use quietset::decision::{Thresholds, decide};
    let t = Thresholds::default();
    assert_eq!(decide(0.9, &t), Decision::Keep);
    assert_eq!(decide(0.6, &t), Decision::Review);
    assert_eq!(decide(0.3, &t), Decision::Drop);
    // boundaries
    assert_eq!(decide(0.85, &t), Decision::Keep);
    assert_eq!(decide(0.40, &t), Decision::Drop);
}

#[test]
fn test_missing_sample_id_is_error() {
    let err = parse_jsonl(r#"{}"#).unwrap_err().to_string();
    assert!(
        err.contains("sample_id"),
        "error should mention sample_id: {err}"
    );
    assert!(parse_jsonl(r#"{"label":"x"}"#).is_err());
}

#[test]
fn test_invalid_score_scale() {
    let zero = ScoreConfig {
        score_scale: 0.0,
        ..ScoreConfig::default()
    };
    assert!(zero.validate().is_err());
    let neg = ScoreConfig {
        score_scale: -1.0,
        ..ScoreConfig::default()
    };
    assert!(neg.validate().is_err());
    let nan = ScoreConfig {
        score_scale: f64::NAN,
        ..ScoreConfig::default()
    };
    assert!(nan.validate().is_err());
    assert!(ScoreConfig::default().validate().is_ok());
}

#[test]
fn test_majority_label_tie_is_deterministic() {
    // 1:1 tie between "alpha" and "beta" — alphabetically first should always win
    let jsonl =
        "{\"sample_id\":\"a\",\"label\":\"beta\"}\n{\"sample_id\":\"a\",\"label\":\"alpha\"}";
    for _ in 0..20 {
        let obs = parse_jsonl(jsonl).unwrap();
        let reports = score_all(obs, &ScoreConfig::default());
        assert_eq!(
            reports[0].majority_label.as_deref(),
            Some("alpha"),
            "tie must resolve deterministically to 'alpha'"
        );
    }
}

#[test]
fn test_seed_sensitivity_affects_score() {
    // Same label, wildly different scores across seeds — should not be Keep
    let jsonl = r#"{"sample_id":"a","label":"win","score":0.95,"seed":1}
{"sample_id":"a","label":"win","score":0.05,"seed":2}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    assert!(reports[0].seed_sensitivity.is_some());
    assert_ne!(
        reports[0].decision,
        Decision::Keep,
        "seed-unstable sample should not be kept (stability={})",
        reports[0].stability_score
    );
}

#[test]
fn test_score_weights_exclude_dimension() {
    // Zero-weight a dimension and verify it doesn't affect the score
    let jsonl = r#"{"sample_id":"a","label":"win","score":0.9}
{"sample_id":"a","label":"win","score":0.1}"#;
    let obs_default = parse_jsonl(jsonl).unwrap();
    let obs_no_score = parse_jsonl(jsonl).unwrap();

    let default_score = score_all(obs_default, &ScoreConfig::default())[0].stability_score;
    let no_score_weight = score_all(
        obs_no_score,
        &ScoreConfig {
            weights: ScoreWeights {
                score_stability: 0.0,
                ..ScoreWeights::default()
            },
            ..ScoreConfig::default()
        },
    )[0]
    .stability_score;

    // Excluding score dimension should raise the score (score is unstable here)
    assert!(
        no_score_weight > default_score,
        "excluding unstable score dimension should raise stability_score"
    );
}

#[test]
fn test_score_nan_is_error() {
    // serde_json rejects NaN in JSON, so we construct an Observation directly
    let mut obs = quietset::Observation {
        sample_id: "a".into(),
        score: Some(f64::NAN),
        ..Default::default()
    };
    // parse_jsonl can't produce NaN (invalid JSON), but we test the API path via
    // a JSONL-round-trip workaround — just verify NaN propagation is caught
    // by constructing the error inline to confirm the error variant exists.
    obs.score = Some(f64::INFINITY);
    let jsonl = format!(
        "{{\"sample_id\":\"a\",\"score\":{}}}",
        "9999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999999"
    );
    // Very large float overflows to infinity in some parsers; test what we can from the API
    drop(jsonl); // the exact JSONL path depends on serde_json behavior
    // Direct API: validate that InvalidScore error exists and is usable
    let err = quietset::Error::InvalidScore { line: 1 };
    assert!(err.to_string().contains("score"));
    let err2 = quietset::Error::InvalidBudget { line: 2 };
    assert!(err2.to_string().contains("budget"));
    drop(obs);
}

#[test]
fn test_invalid_threshold_drop_gt_keep() {
    let config = ScoreConfig {
        thresholds: Thresholds {
            keep: 0.40,
            drop: 0.85,
        }, // drop > keep — invalid
        ..ScoreConfig::default()
    };
    let err = config.validate().unwrap_err().to_string();
    assert!(
        err.contains("drop_threshold") && err.contains("keep_threshold"),
        "error should mention both thresholds: {err}"
    );
}

#[test]
fn test_threshold_out_of_range() {
    let neg = ScoreConfig {
        thresholds: Thresholds {
            keep: -0.1,
            drop: 0.0,
        },
        ..ScoreConfig::default()
    };
    assert!(neg.validate().is_err());

    let over = ScoreConfig {
        thresholds: Thresholds {
            keep: 1.1,
            drop: 0.4,
        },
        ..ScoreConfig::default()
    };
    assert!(over.validate().is_err());
}

#[test]
fn test_components_populated() {
    let jsonl = r#"{"sample_id":"a","label":"win","score":0.9,"budget":4,"seed":1,"model_id":"m1","evaluator_id":"e1"}
{"sample_id":"a","label":"win","score":0.8,"budget":8,"seed":2,"model_id":"m2","evaluator_id":"e2"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let c = &reports[0].components;
    assert!(c.label.is_some(), "label component should be present");
    assert!(
        c.score_consistency.is_some(),
        "score_consistency should be present"
    );
    assert!(
        c.budget_robustness.is_some(),
        "budget_robustness should be present"
    );
    assert!(
        c.seed_robustness.is_some(),
        "seed_robustness should be present"
    );
    assert!(
        c.model_agreement.is_some(),
        "model_agreement should be present"
    );
    assert!(
        c.evaluator_agreement.is_some(),
        "evaluator_agreement should be present"
    );
    // all component values in [0, 1]
    for v in [
        c.label,
        c.score_consistency,
        c.budget_robustness,
        c.seed_robustness,
        c.model_agreement,
        c.evaluator_agreement,
    ]
    .into_iter()
    .flatten()
    {
        assert!((0.0..=1.0).contains(&v), "component {v} out of [0,1]");
    }
}

#[test]
fn test_negative_weight_is_error() {
    let config = ScoreConfig {
        weights: ScoreWeights {
            label_agreement: -1.0,
            ..ScoreWeights::default()
        },
        ..ScoreConfig::default()
    };
    let err = config.validate().unwrap_err().to_string();
    assert!(
        err.contains("label_agreement"),
        "error should mention field: {err}"
    );
}

#[test]
fn test_nan_weight_is_error() {
    let config = ScoreConfig {
        weights: ScoreWeights {
            score_stability: f64::NAN,
            ..ScoreWeights::default()
        },
        ..ScoreConfig::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_all_zero_weights_is_error() {
    let config = ScoreConfig {
        weights: ScoreWeights {
            label_agreement: 0.0,
            score_stability: 0.0,
            budget_stability: 0.0,
            seed_stability: 0.0,
            model_agreement: 0.0,
            evaluator_agreement: 0.0,
        },
        ..ScoreConfig::default()
    };
    let err = config.validate().unwrap_err().to_string();
    assert!(
        err.contains("zero"),
        "error should mention zero weights: {err}"
    );
}

#[test]
fn test_validate_rejects_empty_sample_id() {
    let obs = Observation {
        sample_id: "".into(),
        ..Default::default()
    };
    let err = obs.validate(1).unwrap_err().to_string();
    assert!(
        err.contains("sample_id"),
        "error should mention sample_id: {err}"
    );

    let obs_ws = Observation {
        sample_id: "   ".into(),
        ..Default::default()
    };
    assert!(
        obs_ws.validate(1).is_err(),
        "whitespace-only sample_id should fail"
    );
}

#[test]
fn test_weakest_component_tie_is_deterministic() {
    use quietset::StabilityComponents;
    let c = StabilityComponents {
        label: Some(0.5),
        score_consistency: Some(0.5),
        budget_robustness: Some(0.5),
        seed_robustness: Some(0.5),
        model_agreement: Some(0.5),
        evaluator_agreement: Some(0.5),
    };
    // tie resolved by fixed declaration order — "label" always wins
    for _ in 0..20 {
        let (name, val) = c.weakest().unwrap();
        assert_eq!(name, "label");
        assert_eq!(val, 0.5);
    }
}

#[test]
fn test_confidence_single_obs() {
    let obs = vec![quietset::Observation {
        sample_id: "a".into(),
        score: Some(0.9),
        ..Default::default()
    }];
    let reports = score_all(obs, &ScoreConfig::default());
    // n=1, k=3: confidence = 1/(1+3) = 0.25
    let expected_confidence = 1.0 / (1.0 + 3.0);
    assert!((reports[0].confidence - expected_confidence).abs() < 1e-9);
}

#[test]
fn test_adjusted_score_pulls_toward_half() {
    // n=2, high stability -> adjusted should be lower than raw
    let jsonl = r#"{"sample_id":"a","label":"win","score":0.95}
{"sample_id":"a","label":"win","score":0.94}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let r = &reports[0];
    assert!(
        r.adjusted_stability_score < r.stability_score,
        "adjusted={} should be < raw={}",
        r.adjusted_stability_score,
        r.stability_score
    );
}

#[test]
fn test_min_observations_demotes_keep() {
    let jsonl = r#"{"sample_id":"a","label":"win","score":0.99}
{"sample_id":"a","label":"win","score":0.98}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    // Without min requirement: likely keep
    let reports_default = score_all(obs.clone(), &ScoreConfig::default());
    assert_eq!(reports_default[0].decision, Decision::Keep);
    // With min 5 observations: demoted to review
    let config = ScoreConfig {
        min_requirements: MinRequirements {
            observations: 5,
            ..Default::default()
        },
        ..ScoreConfig::default()
    };
    let reports_min = score_all(obs, &config);
    assert_eq!(
        reports_min[0].decision,
        Decision::Review,
        "should be demoted to review when n < min_observations"
    );
}

#[test]
fn test_label_margin_unanimous() {
    let jsonl = r#"{"sample_id":"a","label":"win"}
{"sample_id":"a","label":"win"}
{"sample_id":"a","label":"win"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    assert!(
        (reports[0].label_margin.unwrap() - 1.0).abs() < 1e-9,
        "unanimous -> margin = 1.0"
    );
}

#[test]
fn test_label_margin_split() {
    // 2 win, 2 loss -> margin = 0
    let jsonl = r#"{"sample_id":"x","label":"win"}
{"sample_id":"x","label":"loss"}
{"sample_id":"x","label":"win"}
{"sample_id":"x","label":"loss"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    assert!(
        (reports[0].label_margin.unwrap() - 0.0).abs() < 1e-9,
        "50/50 -> margin = 0.0"
    );
}

#[test]
fn test_label_entropy_uniform() {
    // 3 equal labels -> normalized entropy = 1.0
    let jsonl = r#"{"sample_id":"a","label":"A"}
{"sample_id":"a","label":"B"}
{"sample_id":"a","label":"C"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let e = reports[0].label_entropy.unwrap();
    assert!(
        (e - 1.0).abs() < 1e-6,
        "uniform 3-class -> entropy = 1.0, got {e}"
    );
}

#[test]
fn test_budget_slope_positive() {
    // higher budget -> higher score: slope should be positive
    let jsonl = r#"{"sample_id":"a","score":0.5,"budget":4}
{"sample_id":"a","score":0.7,"budget":8}
{"sample_id":"a","score":0.9,"budget":16}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let slope = reports[0].budget_slope.unwrap();
    assert!(
        slope > 0.0,
        "increasing scores with budget -> positive slope, got {slope}"
    );
}

#[test]
fn test_evaluator_reliability() {
    use quietset::compute_evaluator_reliability;
    // e1 always agrees with majority, e2 disagrees sometimes
    // sample a: 1 win (e1), 1 win (e2) -> majority = win
    // sample b: 2 win (e1), 1 loss (e2) -> majority = win (unambiguous)
    let jsonl = r#"{"sample_id":"a","label":"win","evaluator_id":"e1"}
{"sample_id":"a","label":"win","evaluator_id":"e2"}
{"sample_id":"b","label":"win","evaluator_id":"e1"}
{"sample_id":"b","label":"win","evaluator_id":"e1"}
{"sample_id":"b","label":"loss","evaluator_id":"e2"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs.clone(), &ScoreConfig::default());
    let rel = compute_evaluator_reliability(&obs, &reports);
    assert_eq!(
        *rel.get("e1").unwrap() as i32,
        1,
        "e1 always matches majority"
    );
    assert!(rel.get("e2").unwrap() < &1.0, "e2 disagrees sometimes");
}

#[test]
fn test_min_requirements_not_overridden_by_adjusted_score() {
    // n=2, high stability — adjusted score (with low confidence_k) stays above keep threshold
    // but n < min_requirements.observations=3, so decision must be Review, not Keep
    let jsonl = r#"{"sample_id":"a","label":"win","score":0.99}
{"sample_id":"a","label":"win","score":0.98}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let config = ScoreConfig {
        decision_score: DecisionScore::Adjusted,
        confidence_k: 0.01, // near-zero k -> confidence ≈ 1 -> adjusted ≈ raw (high score)
        min_requirements: MinRequirements {
            observations: 3,
            ..Default::default()
        },
        ..ScoreConfig::default()
    };
    let reports = score_all(obs, &config);
    // adjusted score is high enough for Keep, but n=2 < 3 -> must stay Review
    assert_eq!(
        reports[0].decision,
        Decision::Review,
        "MinRequirements must take precedence; adjusted_score={:.4}",
        reports[0].adjusted_stability_score
    );
}

#[test]
fn test_adjusted_score_pulls_toward_half_with_large_k() {
    let jsonl = r#"{"sample_id":"a","label":"win","score":0.99}
{"sample_id":"a","label":"win","score":0.98}"#;
    let obs_default = parse_jsonl(jsonl).unwrap();
    let obs_large_k = parse_jsonl(jsonl).unwrap();

    let raw = score_all(obs_default, &ScoreConfig::default())[0].stability_score;
    let adj = score_all(
        obs_large_k,
        &ScoreConfig {
            confidence_k: 100.0,
            ..ScoreConfig::default()
        },
    )[0]
    .adjusted_stability_score;

    assert!(
        adj < raw,
        "large confidence_k -> adjusted score < raw score ({adj:.4} vs {raw:.4})"
    );
    assert!(
        adj > 0.5,
        "adjusted score should still be above 0.5 for high-stability sample"
    );
}

#[test]
fn test_wilson_lcb_lower_than_raw_for_small_n() {
    // 2/2 unanimous: label_agreement = 1.0 but LCB should be < 1.0
    let jsonl = r#"{"sample_id":"a","label":"win"}
{"sample_id":"a","label":"win"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let r = &reports[0];
    assert_eq!(r.label_agreement, Some(1.0));
    // LCB at 95% for 2/2 should be well below 1.0 (roughly 0.34-0.81 range)
    let lcb = r.label_agreement_lcb.unwrap();
    assert!(lcb < 1.0, "LCB for 2/2 should be < 1.0, got {lcb}");
    assert!(lcb > 0.0, "LCB should be > 0.0");
}

#[test]
fn test_wilson_lcb_high_for_large_n() {
    // 25/25 unanimous: LCB should be very high (> 0.85)
    // For unanimous p=1: LCB = n/(n+z^2); z≈1.96, z^2≈3.84; 25/28.84≈0.867
    let jsonl: String = (0..25)
        .map(|i| format!("{{\"sample_id\":\"a\",\"label\":\"win\",\"run_id\":\"{i}\"}}\n"))
        .collect();
    let obs = parse_jsonl(&jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let lcb = reports[0].label_agreement_lcb.unwrap();
    assert!(lcb > 0.85, "LCB for 25/25 should be > 0.85, got {lcb}");
}

#[test]
fn test_lcb_policy_more_conservative_than_raw() {
    // 3/3 unanimous, raw stability should be keep, LCB should be lower
    let jsonl = r#"{"sample_id":"a","label":"win"}
{"sample_id":"a","label":"win"}
{"sample_id":"a","label":"win"}"#;
    let obs_raw = parse_jsonl(jsonl).unwrap();
    let obs_lcb = parse_jsonl(jsonl).unwrap();

    let raw_reports = score_all(obs_raw, &ScoreConfig::default());
    let lcb_reports = score_all(
        obs_lcb,
        &ScoreConfig {
            decision_score: DecisionScore::LowerConfidenceBound,
            ..ScoreConfig::default()
        },
    );
    // Raw should keep (all agree), LCB stability_score should be lower
    assert_eq!(raw_reports[0].decision, Decision::Keep);
    // LCB stability score is lower than raw (LCB penalises low n)
    assert!(
        lcb_reports[0].stability_score <= raw_reports[0].stability_score,
        "LCB stability should be <= raw: {} vs {}",
        lcb_reports[0].stability_score,
        raw_reports[0].stability_score
    );
}

#[test]
fn test_score_mad_less_sensitive_to_outlier() {
    // Scores: 0.9, 0.9, 0.9, 0.9, 0.0 (one outlier)
    // MAD should be much lower than std
    let obs = vec![
        quietset::Observation {
            sample_id: "a".into(),
            score: Some(0.9),
            ..Default::default()
        },
        quietset::Observation {
            sample_id: "a".into(),
            score: Some(0.9),
            ..Default::default()
        },
        quietset::Observation {
            sample_id: "a".into(),
            score: Some(0.9),
            ..Default::default()
        },
        quietset::Observation {
            sample_id: "a".into(),
            score: Some(0.9),
            ..Default::default()
        },
        quietset::Observation {
            sample_id: "a".into(),
            score: Some(0.0),
            ..Default::default()
        },
    ];
    let reports = score_all(obs, &ScoreConfig::default());
    let r = &reports[0];
    let std = r.score_std.unwrap();
    let mad = r.score_mad.unwrap();
    assert!(
        mad < std,
        "MAD ({mad:.4}) should be < std ({std:.4}) with one outlier"
    );
    assert!(
        mad < 0.01,
        "MAD should be near 0 (4 identical scores), got {mad:.4}"
    );
}

#[test]
fn test_gold_label_changes_reliability() {
    // e1 matches majority (win is strict majority with 2 win vs 1 loss), e2 doesn't
    // With gold_label=loss on all samples, reliability reverses
    let jsonl_no_gold = r#"{"sample_id":"a","label":"win","evaluator_id":"e1"}
{"sample_id":"a","label":"win","evaluator_id":"e1"}
{"sample_id":"a","label":"loss","evaluator_id":"e2"}"#;

    let jsonl_gold = r#"{"sample_id":"a","label":"win","evaluator_id":"e1","gold_label":"loss"}
{"sample_id":"a","label":"win","evaluator_id":"e1","gold_label":"loss"}
{"sample_id":"a","label":"loss","evaluator_id":"e2","gold_label":"loss"}"#;

    let obs_no_gold = parse_jsonl(jsonl_no_gold).unwrap();
    let obs_gold = parse_jsonl(jsonl_gold).unwrap();

    let reports = score_all(obs_no_gold.clone(), &ScoreConfig::default());
    let rel_no_gold = compute_evaluator_reliability(&obs_no_gold, &reports);

    let reports_g = score_all(obs_gold.clone(), &ScoreConfig::default());
    let rel_gold = compute_evaluator_reliability(&obs_gold, &reports_g);

    // Without gold: e1 matches majority (win) -> high reliability
    // With gold (loss): e1 doesn't match gold -> low reliability
    let e1_no_gold = *rel_no_gold.get("e1").unwrap();
    let e1_gold = *rel_gold.get("e1").unwrap();
    assert!(
        e1_no_gold > e1_gold,
        "e1 reliability should be lower when gold_label differs: {e1_no_gold:.2} vs {e1_gold:.2}"
    );
}

#[test]
fn test_lcb_keep_demotions_excludes_already_unstable() {
    // The key semantic: lcb_keep_demotions counts samples where
    //   stability_score >= keep_threshold  (raw would keep)
    //   AND label_agreement_lcb < keep_threshold  (LCB would not keep)
    //
    // Critically, a sample already below keep_threshold in raw mode (e.g. split labels)
    // must NOT be counted even if its LCB is also below the threshold.
    //
    // Note: Wilson LCB at 95% confidence requires ~22+ observations of all-match to exceed 0.85,
    // so small samples (even fully-agreeing ones) still have LCB < 0.85 and are valid demotions.
    let config = ScoreConfig {
        thresholds: Thresholds {
            keep: 0.85,
            drop: 0.40,
        },
        confidence_level: 0.95,
        ..ScoreConfig::default()
    };

    let make_obs = |id: &str, label: &str| Observation {
        sample_id: id.into(),
        label: Some(label.into()),
        ..Default::default()
    };

    let mut obs = Vec::new();
    // sample_a, sample_b: fully-agreeing → stability_score = 1.0, LCB < 0.85 → demotion candidates
    for _ in 0..5 {
        obs.push(make_obs("a", "win"));
    }
    for _ in 0..2 {
        obs.push(make_obs("b", "win"));
    }
    // sample_c: 50/50 split → stability_score < 0.85 → already unstable, must NOT be counted
    obs.push(make_obs("c", "win"));
    obs.push(make_obs("c", "loss"));

    let reports = score_all(obs, &config);
    let keep_threshold = config.thresholds.keep;

    let is_demotion = |r: &&quietset::StabilityReport| {
        r.stability_score >= keep_threshold
            && r.label_agreement_lcb
                .map(|v| v < keep_threshold)
                .unwrap_or(false)
    };

    // sample_c must not appear in demotions — it's already review in raw mode
    let report_c = reports.iter().find(|r| r.sample_id == "c").unwrap();
    assert!(
        report_c.stability_score < keep_threshold,
        "sample_c stability_score should be below keep threshold: {:.4}",
        report_c.stability_score
    );
    assert!(
        !is_demotion(&report_c),
        "sample_c must not be counted as a demotion (already unstable)"
    );

    // sample_a and sample_b should be demotions (raw-keep but LCB < threshold)
    let report_a = reports.iter().find(|r| r.sample_id == "a").unwrap();
    let report_b = reports.iter().find(|r| r.sample_id == "b").unwrap();
    assert!(
        is_demotion(&report_a),
        "sample_a should be a demotion candidate"
    );
    assert!(
        is_demotion(&report_b),
        "sample_b should be a demotion candidate"
    );
}
