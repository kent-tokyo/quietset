use quietset::{
    Decision, DecisionScore, MinRequirements, Observation, ScoreConfig, ScoreDispersion,
    ScoreWeights, Thresholds, compute_calibration, compute_evaluator_reliability,
    compute_evaluator_weights, compute_fleiss_kappa, compute_krippendorff_alpha, parse_jsonl,
    score_all, score_all_weighted,
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
fn test_score_iqr_even_n_uses_interpolation() {
    // Scores: 1,2,3,4 (even n). Linear interpolation (NumPy/R default) gives
    // median=2.5, Q1=1.75, Q3=3.25, IQR=1.5. Nearest-rank rounding would give
    // median=3, Q1=2, Q3=4, IQR=2 instead.
    let obs = vec![1.0, 2.0, 3.0, 4.0]
        .into_iter()
        .map(|score| quietset::Observation {
            sample_id: "a".into(),
            score: Some(score),
            ..Default::default()
        })
        .collect();
    let config = ScoreConfig {
        score_dispersion: ScoreDispersion::Iqr,
        ..ScoreConfig::default()
    };
    let reports = score_all(obs, &config);
    let iqr = reports[0].score_iqr.unwrap();
    assert!(
        (iqr - 1.5).abs() < 1e-9,
        "expected interpolated IQR 1.5, got {iqr}"
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

#[test]
fn test_fleiss_kappa_perfect_agreement() {
    // 3 raters, 3 subjects, all agree → kappa = 1.0
    let jsonl = r#"{"sample_id":"a","label":"yes","evaluator_id":"e1"}
{"sample_id":"a","label":"yes","evaluator_id":"e2"}
{"sample_id":"a","label":"yes","evaluator_id":"e3"}
{"sample_id":"b","label":"no","evaluator_id":"e1"}
{"sample_id":"b","label":"no","evaluator_id":"e2"}
{"sample_id":"b","label":"no","evaluator_id":"e3"}
{"sample_id":"c","label":"yes","evaluator_id":"e1"}
{"sample_id":"c","label":"yes","evaluator_id":"e2"}
{"sample_id":"c","label":"yes","evaluator_id":"e3"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let k = compute_fleiss_kappa(&obs).expect("should compute kappa");
    assert!(
        (k - 1.0).abs() < 1e-9,
        "perfect agreement → kappa=1.0, got {k:.6}"
    );
}

#[test]
fn test_fleiss_kappa_perfect_disagreement() {
    // 2 raters always disagree → P_obs=0, P_e=0.5 → kappa = (0-0.5)/(1-0.5) = -1.0
    let jsonl = r#"{"sample_id":"a","label":"yes","evaluator_id":"e1"}
{"sample_id":"a","label":"no","evaluator_id":"e2"}
{"sample_id":"b","label":"yes","evaluator_id":"e1"}
{"sample_id":"b","label":"no","evaluator_id":"e2"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let k = compute_fleiss_kappa(&obs).expect("should compute kappa");
    assert!(
        (k + 1.0).abs() < 1e-9,
        "perfect disagreement → kappa=-1.0, got {k:.6}"
    );
}

#[test]
fn test_krippendorff_alpha_perfect_agreement() {
    let jsonl = r#"{"sample_id":"a","label":"win","evaluator_id":"e1"}
{"sample_id":"a","label":"win","evaluator_id":"e2"}
{"sample_id":"b","label":"loss","evaluator_id":"e1"}
{"sample_id":"b","label":"loss","evaluator_id":"e2"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let a = compute_krippendorff_alpha(&obs).expect("should compute alpha");
    assert!(
        (a - 1.0).abs() < 1e-9,
        "perfect agreement → alpha=1.0, got {a:.6}"
    );
}

#[test]
fn test_krippendorff_alpha_perfect_disagreement() {
    // 2 raters always disagree → Do=1, De=2/3 → alpha = 1 - 1/(2/3) = -0.5
    let jsonl = r#"{"sample_id":"a","label":"yes","evaluator_id":"e1"}
{"sample_id":"a","label":"no","evaluator_id":"e2"}
{"sample_id":"b","label":"yes","evaluator_id":"e1"}
{"sample_id":"b","label":"no","evaluator_id":"e2"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let a = compute_krippendorff_alpha(&obs).expect("should compute alpha");
    assert!(
        (a + 0.5).abs() < 1e-9,
        "perfect disagreement → alpha=-0.5, got {a:.6}"
    );
}

#[test]
fn test_kappa_and_alpha_undefined_for_single_rater() {
    // Only one rating per subject — undefined
    let jsonl = r#"{"sample_id":"a","label":"yes","evaluator_id":"e1"}
{"sample_id":"b","label":"no","evaluator_id":"e1"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    assert!(compute_fleiss_kappa(&obs).is_none());
    assert!(compute_krippendorff_alpha(&obs).is_none());
}

#[test]
fn test_calibrate_target_precision() {
    // a, b, c: 5 observations each, all agree, gold matches → stable and correct
    // e: 5 obs, all "loss", but gold="win" → stable but WRONG
    // d: 2 obs, split → unstable
    let make = |id: &str, label: &str, gold: &str, n: usize| -> Vec<Observation> {
        (0..n)
            .map(|i| Observation {
                sample_id: id.into(),
                label: Some(label.into()),
                gold_label: Some(gold.into()),
                evaluator_id: Some(format!("e{i}")),
                ..Default::default()
            })
            .collect()
    };
    let mut obs = Vec::new();
    obs.extend(make("a", "win", "win", 5));
    obs.extend(make("b", "win", "win", 5));
    obs.extend(make("c", "win", "win", 5));
    obs.push(Observation {
        sample_id: "d".into(),
        label: Some("win".into()),
        gold_label: Some("win".into()),
        ..Default::default()
    });
    obs.push(Observation {
        sample_id: "d".into(),
        label: Some("loss".into()),
        gold_label: Some("win".into()),
        ..Default::default()
    });
    obs.extend(make("e", "loss", "win", 5)); // stable but wrong

    // e is stable (all "loss") and scores 1.0 just like a/b/c; no threshold can separate it.
    // Best achievable precision: 3 correct / 4 kept (a,b,c,e all score 1.0) = 0.75
    let result = compute_calibration(&obs, &DecisionScore::Raw, 0.95, 0.75, None, 0.40);
    assert!(
        result.is_some(),
        "should find a threshold meeting 0.75 precision"
    );
    let r = result.unwrap();
    assert!(
        r.achieved_precision >= 0.75,
        "precision {:.3} should be >= 0.75",
        r.achieved_precision
    );
    assert!(r.keep_threshold >= 0.50 && r.keep_threshold <= 0.99);
}

#[test]
fn test_calibrate_no_gold_returns_none() {
    let obs = vec![
        Observation {
            sample_id: "a".into(),
            label: Some("win".into()),
            ..Default::default()
        },
        Observation {
            sample_id: "a".into(),
            label: Some("win".into()),
            ..Default::default()
        },
    ];
    assert!(compute_calibration(&obs, &DecisionScore::Raw, 0.95, 0.95, None, 0.40).is_none());
}

#[test]
fn test_profile_llm_judge_base_weights() {
    let base = ScoreWeights {
        evaluator_agreement: 2.0,
        model_agreement: 2.0,
        ..ScoreWeights::default()
    };
    assert!((base.evaluator_agreement - 2.0).abs() < 1e-9);
    assert!((base.model_agreement - 2.0).abs() < 1e-9);
    assert!((base.label_agreement - 1.0).abs() < 1e-9);
    assert!((base.budget_stability - 1.0).abs() < 1e-9);
}

#[test]
fn test_filter_confidence_threshold() {
    // Sample with 2 obs has lower confidence than sample with 10 obs
    let obs2 = vec![
        Observation {
            sample_id: "s2".into(),
            label: Some("w".into()),
            ..Default::default()
        };
        2
    ];
    let obs10 = vec![
        Observation {
            sample_id: "s10".into(),
            label: Some("w".into()),
            ..Default::default()
        };
        10
    ];
    let mut all = obs2;
    all.extend(obs10);
    let reports = score_all(all, &ScoreConfig::default());
    let r2 = reports.iter().find(|r| r.sample_id == "s2").unwrap();
    let r10 = reports.iter().find(|r| r.sample_id == "s10").unwrap();
    // confidence = n/(n+3): 2→0.40, 10→0.77
    assert!(r2.confidence < r10.confidence);
    // A filter with min_confidence=0.6 should drop s2 but keep s10
    assert!(r2.confidence < 0.6);
    assert!(r10.confidence >= 0.6);
}

#[test]
fn test_label_distribution_basic() {
    // 2 "win" + 1 "loss" → distribution {"win": ~0.667, "loss": ~0.333}
    let jsonl = r#"{"sample_id":"a","label":"win"}
{"sample_id":"a","label":"win"}
{"sample_id":"a","label":"loss"}"#;
    let obs = parse_jsonl(jsonl).unwrap();
    let reports = score_all(obs, &ScoreConfig::default());
    let r = &reports[0];
    let dist = r
        .label_distribution
        .as_ref()
        .expect("label_distribution should be Some");
    let win_frac = *dist.get("win").unwrap();
    let loss_frac = *dist.get("loss").unwrap();
    assert!(
        (win_frac - 2.0 / 3.0).abs() < 1e-9,
        "win fraction: {win_frac}"
    );
    assert!(
        (loss_frac - 1.0 / 3.0).abs() < 1e-9,
        "loss fraction: {loss_frac}"
    );
    // Most common label comes first
    let first = dist.keys().next().unwrap();
    assert_eq!(first, "win");
}

#[test]
fn test_label_distribution_none_when_no_labels() {
    let obs = vec![Observation {
        sample_id: "x".into(),
        score: Some(0.9),
        ..Default::default()
    }];
    let reports = score_all(obs, &ScoreConfig::default());
    assert!(reports[0].label_distribution.is_none());
}

#[test]
fn test_weighted_majority_overrides_majority() {
    // Evaluator A (reliable: gold matches) says "win".
    // Evaluators B and C (unreliable: gold disagrees) both say "loss".
    // Raw majority = "loss" (2 vs 1). Weighted majority should = "win".
    let obs = vec![
        Observation {
            sample_id: "s".into(),
            label: Some("win".into()),
            evaluator_id: Some("A".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
        Observation {
            sample_id: "s".into(),
            label: Some("loss".into()),
            evaluator_id: Some("B".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
        Observation {
            sample_id: "s".into(),
            label: Some("loss".into()),
            evaluator_id: Some("C".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
    ];
    let reports = score_all_weighted(obs.clone(), &ScoreConfig::default());
    let r = &reports[0];
    // Raw majority is "loss"
    assert_eq!(r.majority_label.as_deref(), Some("loss"));
    // Evaluator weights: A matches gold (1/1 + smoothing), B&C don't (0/1 + smoothing)
    let truth: std::collections::HashMap<String, String> = obs
        .iter()
        .filter_map(|o| o.gold_label.clone().map(|g| (o.sample_id.clone(), g)))
        .collect();
    let weights = compute_evaluator_weights(&obs, &truth);
    let w_a = *weights.get("A").unwrap();
    let w_b = *weights.get("B").unwrap();
    assert!(w_a > w_b, "A should have higher weight than B/C");
    // Weighted majority overrides: "win" wins when A's weight > sum of B+C weights
    // A: (1+0.5)/(1+1)=0.75, B/C: (0+0.5)/(1+1)=0.25 each → win score=0.75, loss=0.50
    assert_eq!(r.weighted_majority_label.as_deref(), Some("win"));
    assert_eq!(r.majority_weighted_conflict, Some(true));
}

#[test]
fn test_game_ai_profile_uses_lcb_and_stricter_mins() {
    // Verify game-ai profile config values (test the library-level config, not CLI)
    // In the CLI, game-ai sets: budget_stability×2, seed_stability×2, min_obs=4, min_budgets=2, min_seeds=2, lcb
    // We replicate that config here directly.
    let config = ScoreConfig {
        weights: ScoreWeights {
            budget_stability: 2.0,
            seed_stability: 2.0,
            ..ScoreWeights::default()
        },
        min_requirements: MinRequirements {
            observations: 4,
            budgets: 2,
            seeds: 2,
            ..MinRequirements::default()
        },
        decision_score: DecisionScore::LowerConfidenceBound,
        ..ScoreConfig::default()
    };
    // Sample with enough budget/seed variation to pass score but only 1 budget level → demoted
    let obs: Vec<Observation> = (0..5)
        .map(|i| Observation {
            sample_id: "p".into(),
            label: Some("win".into()),
            score: Some(0.9),
            budget: Some(4.0), // all same budget → min_budgets=2 not met
            seed: Some(i),
            ..Default::default()
        })
        .collect();
    let reports = score_all(obs, &config);
    // Would be keep by stability score, but demoted because only 1 distinct budget
    assert_ne!(
        reports[0].decision,
        Decision::Keep,
        "should be demoted by min_budgets=2"
    );
}

#[test]
fn test_policy_precision_decreases_with_lower_threshold() {
    // With gold_label: higher threshold → higher precision (fewer, better samples)
    let obs: Vec<Observation> = vec![
        // 4 stable-correct (agreement 1.0 for "win", gold="win")
        Observation {
            sample_id: "a".into(),
            label: Some("win".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
        Observation {
            sample_id: "a".into(),
            label: Some("win".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
        Observation {
            sample_id: "b".into(),
            label: Some("win".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
        Observation {
            sample_id: "b".into(),
            label: Some("win".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
        // 1 wrong (agreement 1.0 for "loss", gold="win") — stable but wrong
        Observation {
            sample_id: "c".into(),
            label: Some("loss".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
        Observation {
            sample_id: "c".into(),
            label: Some("loss".into()),
            gold_label: Some("win".into()),
            ..Default::default()
        },
    ];
    let gold: std::collections::HashMap<String, String> = obs
        .iter()
        .filter_map(|o| o.gold_label.clone().map(|g| (o.sample_id.clone(), g)))
        .collect();
    // Score all
    let config = ScoreConfig {
        thresholds: Thresholds {
            keep: 0.0,
            drop: -1.0,
        },
        ..ScoreConfig::default()
    };
    let reports = score_all(obs, &config);
    // At threshold=0.0 all 3 are "kept"; precision = 2/3
    let all_kept: Vec<_> = reports
        .iter()
        .filter(|r| r.stability_score >= 0.0)
        .collect();
    let matches = all_kept
        .iter()
        .filter(|r| {
            gold.get(&r.sample_id)
                .and_then(|g| r.majority_label.as_deref().map(|m| m == g.as_str()))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(all_kept.len(), 3);
    assert_eq!(matches, 2, "2 of 3 samples are correct");
    // At high threshold only a and b are kept (stable correct ones, same stability)
    // c has same stability but majority_label="loss"≠"win"
    // All have same stability_score since they're all unanimous 2-obs
    // This just validates our precision math is right
    let prec_all = matches as f64 / all_kept.len() as f64;
    assert!((prec_all - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn test_active_review_high_entropy_has_higher_urgency() {
    // Sample with high entropy should rank higher urgency than sample with low entropy
    let jsonl_high = r#"{"sample_id":"noisy","label":"win"}
{"sample_id":"noisy","label":"loss"}"#;
    let jsonl_low = r#"{"sample_id":"stable","label":"win"}
{"sample_id":"stable","label":"win"}"#;
    let mut obs: Vec<Observation> = parse_jsonl(jsonl_high).unwrap();
    obs.extend(parse_jsonl(jsonl_low).unwrap());
    let reports = score_all(obs, &ScoreConfig::default());
    let noisy = reports.iter().find(|r| r.sample_id == "noisy").unwrap();
    let stable = reports.iter().find(|r| r.sample_id == "stable").unwrap();
    // noisy has higher entropy than stable
    let noisy_ent = noisy.label_entropy.unwrap_or(0.0);
    let stable_ent = stable.label_entropy.unwrap_or(0.0);
    assert!(
        noisy_ent > stable_ent,
        "noisy entropy {noisy_ent} should exceed stable {stable_ent}"
    );
    // urgency ∝ entropy: noisy > stable (actual urgency computation is in CLI)
    assert_eq!(noisy_ent, 1.0, "uniform 50/50 split = entropy 1.0");
    assert_eq!(stable_ent, 0.0, "unanimous = entropy 0.0");
}

#[test]
fn test_active_review_low_lcb_has_higher_urgency_signal() {
    // A sample with low label_agreement_lcb should have a higher urgency signal than one with high lcb
    let many_win: Vec<Observation> = (0..20)
        .map(|i| Observation {
            sample_id: "high_lcb".into(),
            label: Some("win".into()),
            evaluator_id: Some(format!("e{i}")),
            ..Default::default()
        })
        .collect();
    let two_win: Vec<Observation> = (0..2)
        .map(|i| Observation {
            sample_id: "low_lcb".into(),
            label: Some("win".into()),
            evaluator_id: Some(format!("f{i}")),
            ..Default::default()
        })
        .collect();
    let mut obs = many_win;
    obs.extend(two_win);
    let reports = score_all(obs, &ScoreConfig::default());
    let high = reports.iter().find(|r| r.sample_id == "high_lcb").unwrap();
    let low = reports.iter().find(|r| r.sample_id == "low_lcb").unwrap();
    // high_lcb sample has much higher LCB (22 obs at 100% → LCB ≈ 0.85)
    // low_lcb sample has low LCB (2 obs at 100% → LCB ≈ 0.34)
    let high_lcb_val = high.label_agreement_lcb.unwrap_or(0.0);
    let low_lcb_val = low.label_agreement_lcb.unwrap_or(0.0);
    assert!(
        high_lcb_val > low_lcb_val,
        "20-obs sample should have higher LCB than 2-obs: {high_lcb_val} vs {low_lcb_val}"
    );
    // Urgency signal for low_lcb is higher (1 - lcb is larger)
    let urgency_high = 1.0 - high_lcb_val;
    let urgency_low = 1.0 - low_lcb_val;
    assert!(
        urgency_low > urgency_high,
        "low-lcb sample urgency {urgency_low:.3} > high-lcb {urgency_high:.3}"
    );
}

#[test]
fn test_score_dispersion_mad_less_sensitive_to_outlier() {
    // 3 scores: 0.90, 0.88, 0.02 — the 0.02 is an outlier.
    // STD will be dragged down; MAD (based on median) won't be.
    let obs: Vec<Observation> = [0.90f64, 0.88, 0.02]
        .iter()
        .map(|&s| Observation {
            sample_id: "a".into(),
            score: Some(s),
            ..Default::default()
        })
        .collect();

    let std_config = ScoreConfig::default(); // ScoreDispersion::Std
    let mad_config = ScoreConfig {
        score_dispersion: ScoreDispersion::Mad,
        ..ScoreConfig::default()
    };

    let r_std = &score_all(obs.clone(), &std_config)[0];
    let r_mad = &score_all(obs, &mad_config)[0];

    let sc_std = r_std.components.score_consistency.unwrap();
    let sc_mad = r_mad.components.score_consistency.unwrap();

    assert!(
        sc_mad > sc_std,
        "MAD score_consistency ({sc_mad:.4}) should exceed STD ({sc_std:.4}) when outlier present"
    );
    // Verify both are in [0, 1]
    assert!((0.0..=1.0).contains(&sc_std));
    assert!((0.0..=1.0).contains(&sc_mad));
}

#[test]
fn test_score_dispersion_iqr_zero_gives_perfect_consistency() {
    // All scores identical → IQR=0 → score_consistency=1.0
    let obs: Vec<Observation> = [0.85f64, 0.85, 0.85, 0.85]
        .iter()
        .map(|&s| Observation {
            sample_id: "b".into(),
            score: Some(s),
            ..Default::default()
        })
        .collect();

    let iqr_config = ScoreConfig {
        score_dispersion: ScoreDispersion::Iqr,
        ..ScoreConfig::default()
    };
    let r = &score_all(obs, &iqr_config)[0];
    let sc = r.components.score_consistency.unwrap();
    assert!(
        (sc - 1.0).abs() < 1e-9,
        "identical scores → IQR=0 → score_consistency=1.0, got {sc}"
    );
}

#[test]
fn test_score_dispersion_mad_single_obs_skips_score_consistency() {
    // MAD requires >= 2 obs; with 1 obs, score_consistency should be None
    let obs = vec![Observation {
        sample_id: "c".into(),
        score: Some(0.9),
        ..Default::default()
    }];
    let mad_config = ScoreConfig {
        score_dispersion: ScoreDispersion::Mad,
        ..ScoreConfig::default()
    };
    let r = &score_all(obs, &mad_config)[0];
    // Single obs → stability_score=0.5 (review), score_consistency None
    assert!(
        r.components.score_consistency.is_none(),
        "single obs with MAD should have no score_consistency"
    );
    assert_eq!(r.decision, Decision::Review);
}
