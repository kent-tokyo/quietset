use std::process::Output;

use assert_cmd::Command;

fn run(args: &[&str]) -> Output {
    Command::cargo_bin("quietset")
        .unwrap()
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("quietset_cli_test_{name}_{}", std::process::id()));
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn score_jsonl_succeeds_on_valid_fixture() {
    let out = run(&["score", "../../tests/fixtures/simple.jsonl"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("\"sample_id\":\"a\""));
}

#[test]
fn score_invalid_jsonl_without_skip_invalid_fails() {
    let path = write_temp(
        "bad.jsonl",
        "{\"sample_id\":\"a\",\"score\":0.5}\nNOT JSON\n",
    );
    let out = run(&["score", path.to_str().unwrap()]);
    assert!(!out.status.success());
}

#[test]
fn score_invalid_jsonl_with_skip_invalid_succeeds_and_warns() {
    let path = write_temp(
        "bad_skip.jsonl",
        "{\"sample_id\":\"a\",\"score\":0.5}\nNOT JSON\n",
    );
    let out = run(&["score", path.to_str().unwrap(), "--skip-invalid"]);
    assert!(out.status.success());
    assert!(stderr(&out).contains("warning: skipping"));
}

// Regression test for the bug fixed in this session: `--skip-invalid` used to be
// silently ignored for `--format csv`, so a single bad row failed the whole run
// regardless of the flag.
#[test]
fn score_csv_skip_invalid_skips_bad_row() {
    let path = write_temp(
        "bad.csv",
        "sample_id,label,score\na,win,0.9\nb,win,not_a_number\nc,win,0.8\n",
    );

    let failing = run(&["score", path.to_str().unwrap(), "--format", "csv"]);
    assert!(!failing.status.success());

    let ok = run(&[
        "score",
        path.to_str().unwrap(),
        "--format",
        "csv",
        "--skip-invalid",
    ]);
    assert!(ok.status.success());
    assert!(stderr(&ok).contains("warning: skipping row 2"));
}

// Regression test for the bug fixed in this session: `explain` used to swallow
// JSONL parse errors with `.ok()`, so a malformed line anywhere in the file made
// unrelated, later sample_ids report a misleading "not found" instead of the
// real parse error.
#[test]
fn explain_reports_parse_error_not_misleading_not_found() {
    let scored = run(&["score", "../../tests/fixtures/simple.jsonl"]);
    assert!(scored.status.success());
    let scored_text = stdout(&scored);
    let lines: Vec<&str> = scored_text.lines().collect();
    assert!(lines.len() >= 2, "fixture should score at least 2 samples");

    let corrupted = format!("{}\nNOT JSON\n{}\n", lines[0], lines[1]);
    let path = write_temp("explain_corrupted.jsonl", &corrupted);

    // sample_id on line 1 is found before the corrupted line is ever read.
    let found_before_corruption = run(&["explain", path.to_str().unwrap(), "--sample-id", "a"]);
    assert!(found_before_corruption.status.success());

    // sample_id on line 3 requires scanning past the corrupted line 2 — this
    // must surface the parse error, not "sample_id 'b' not found".
    let must_scan_past_corruption = run(&["explain", path.to_str().unwrap(), "--sample-id", "b"]);
    assert!(!must_scan_past_corruption.status.success());
    assert!(stderr(&must_scan_past_corruption).contains("parsing line 2"));
}

#[test]
fn explain_missing_sample_id_without_corruption_reports_not_found() {
    let scored = run(&["score", "../../tests/fixtures/simple.jsonl"]);
    assert!(scored.status.success());
    let path = write_temp("scored_ok.jsonl", &stdout(&scored));

    let out = run(&["explain", path.to_str().unwrap(), "--sample-id", "zzz"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("not found"));
}

#[test]
fn score_then_filter_pipeline_via_files() {
    let scored = run(&["score", "../../tests/fixtures/simple.jsonl"]);
    assert!(scored.status.success());
    let path = write_temp("pipeline.jsonl", &stdout(&scored));

    let out = run(&["filter", path.to_str().unwrap(), "--decision", "keep"]);
    assert!(out.status.success());
}

// Regression test for the bug fixed in this session: `filter` had no empty-result
// guard, so if every input row was invalid and dropped via `--skip-invalid`, it
// silently produced 0 lines of output with exit code 0 instead of erroring like
// `summary`/`audit`/`select`/`recommend` already do.
#[test]
fn filter_all_invalid_with_skip_invalid_fails_instead_of_silently_succeeding() {
    let path = write_temp("filter_all_bad.jsonl", "NOT JSON\nALSO NOT JSON\n");
    let out = run(&["filter", path.to_str().unwrap(), "--skip-invalid"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no records found"));
}

fn gold_labeled_fixture() -> std::path::PathBuf {
    write_temp(
        "gold_labeled.jsonl",
        "{\"sample_id\":\"a\",\"label\":\"win\",\"score\":0.9,\"gold_label\":\"win\"}\n\
         {\"sample_id\":\"a\",\"label\":\"win\",\"score\":0.9,\"gold_label\":\"win\"}\n\
         {\"sample_id\":\"a\",\"label\":\"win\",\"score\":0.9,\"gold_label\":\"win\"}\n\
         {\"sample_id\":\"b\",\"label\":\"win\",\"score\":0.9,\"gold_label\":\"win\"}\n\
         {\"sample_id\":\"b\",\"label\":\"win\",\"score\":0.9,\"gold_label\":\"win\"}\n\
         {\"sample_id\":\"b\",\"label\":\"win\",\"score\":0.9,\"gold_label\":\"win\"}\n",
    )
}

#[test]
fn calibrate_csv_output_has_header_and_one_data_row() {
    let path = gold_labeled_fixture();
    let out = run(&[
        "calibrate",
        path.to_str().unwrap(),
        "--target-precision",
        "0.9",
        "--output-format",
        "csv",
    ]);
    assert!(out.status.success());
    let text = stdout(&out);
    let mut lines = text.lines();
    let header = lines.next().expect("header row");
    assert_eq!(
        header,
        "decision_score,keep_threshold,drop_threshold,achieved_precision,precision_ci_low,precision_ci_high,coverage,n_keep,n_total,train_precision,validated_on_heldout,note"
    );
    let data = lines.next().expect("one data row");
    assert_eq!(data.split(',').count(), 12);
    assert!(lines.next().is_none(), "should be exactly one data row");
}

#[test]
fn policy_csv_output_has_header_and_fixed_columns() {
    let path = gold_labeled_fixture();
    let out = run(&["policy", path.to_str().unwrap(), "--output-format", "csv"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let mut lines = text.lines();
    let header = lines.next().expect("header row");
    assert_eq!(
        header,
        "threshold,n_keep,coverage,precision,stable_wrong_rate,best"
    );
    let data_rows: Vec<&str> = lines.collect();
    assert_eq!(data_rows.len(), 50, "one row per swept threshold");
    for row in &data_rows {
        assert_eq!(row.split(',').count(), 6);
    }
}

#[test]
fn active_review_output_has_expected_value_fields_and_renamed_actions() {
    let path = write_temp(
        "active_review_input.jsonl",
        "{\"sample_id\":\"a\",\"label\":\"win\"}\n\
         {\"sample_id\":\"a\",\"label\":\"loss\"}\n",
    );
    let scored = run(&["score", path.to_str().unwrap()]);
    assert!(scored.status.success());
    let scored_path = write_temp("active_review_scored.jsonl", &stdout(&scored));

    let out = run(&["active-review", scored_path.to_str().unwrap()]);
    assert!(out.status.success());
    let text = stdout(&out);
    let line = text.lines().next().expect("one output line");
    let value: serde_json::Value = serde_json::from_str(line).unwrap();

    for key in [
        "sample_id",
        "urgency_score",
        "primary_reason",
        "suggested_action",
        "expected_coverage_gain",
        "expected_risk_reduction",
        "cost",
        "utility",
    ] {
        assert!(value.get(key).is_some(), "missing field: {key}");
    }
    let action = value["suggested_action"].as_str().unwrap();
    assert!(
        [
            "request_gold_label",
            "add_evaluator",
            "add_model",
            "increase_budget",
            "add_seed"
        ]
        .contains(&action),
        "suggested_action should use the renamed action set, got {action}"
    );
}

#[test]
fn calibrate_fail_below_target_exits_nonzero_but_still_prints_output() {
    // Training: 9/10 correct (0.90 precision) -> a threshold clears --target-precision 0.80.
    // Held-out: 2/10 correct (0.20 precision) -> falls well short of the target.
    let mut train = String::new();
    for i in 0..9 {
        train.push_str(&format!(
            "{{\"sample_id\":\"t{i}\",\"label\":\"win\",\"gold_label\":\"win\",\"evaluator_id\":\"e0\"}}\n"
        ));
    }
    train.push_str(
        "{\"sample_id\":\"tw\",\"label\":\"loss\",\"gold_label\":\"win\",\"evaluator_id\":\"e0\"}\n",
    );
    let train_path = write_temp("calibrate_fail_train.jsonl", &train);

    let mut heldout = String::new();
    for i in 0..2 {
        heldout.push_str(&format!(
            "{{\"sample_id\":\"h{i}\",\"label\":\"win\",\"gold_label\":\"win\",\"evaluator_id\":\"e0\"}}\n"
        ));
    }
    for i in 0..8 {
        heldout.push_str(&format!(
            "{{\"sample_id\":\"hw{i}\",\"label\":\"loss\",\"gold_label\":\"win\",\"evaluator_id\":\"e0\"}}\n"
        ));
    }
    let heldout_path = write_temp("calibrate_fail_heldout.jsonl", &heldout);

    let out = run(&[
        "calibrate",
        train_path.to_str().unwrap(),
        "--target-precision",
        "0.80",
        "--heldout",
        heldout_path.to_str().unwrap(),
        "--fail-below-target",
    ]);
    assert!(
        !out.status.success(),
        "should exit non-zero when held-out precision falls below target"
    );
    assert!(
        stdout(&out).contains("\"achieved_precision\""),
        "the calibration result should still be printed to stdout before the non-zero exit"
    );
    assert!(stderr(&out).contains("below --target-precision"));
}

#[test]
fn active_review_plan_respects_budget_and_defaults_to_unbounded() {
    let path = write_temp(
        "plan_input.jsonl",
        "{\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"win\"}\n\
         {\"sample_id\":\"cheap\",\"label\":\"loss\"}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":0,\"score\":0.9}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":1,\"score\":0.1}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":0,\"score\":0.9}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":1,\"score\":0.1}\n",
    );
    let scored = run(&["score", path.to_str().unwrap()]);
    assert!(scored.status.success());
    let scored_path = write_temp("plan_scored.jsonl", &stdout(&scored));

    let plan_path =
        std::env::temp_dir().join(format!("quietset_cli_test_plan_out_{}", std::process::id()));

    let out = run(&[
        "active-review",
        scored_path.to_str().unwrap(),
        "--plan",
        plan_path.to_str().unwrap(),
        "--budget",
        "1.0",
    ]);
    assert!(out.status.success());
    let plan_text = std::fs::read_to_string(&plan_path).unwrap();
    let plan_lines: Vec<&str> = plan_text.lines().collect();
    assert_eq!(
        plan_lines.len(),
        1,
        "budget 1.0 should only afford the single highest-utility (cost 1.0) entry"
    );
    assert!(plan_lines[0].contains("\"sample_id\":\"cheap\""));

    let out_unbounded = run(&[
        "active-review",
        scored_path.to_str().unwrap(),
        "--plan",
        plan_path.to_str().unwrap(),
    ]);
    assert!(out_unbounded.status.success());
    let unbounded_text = std::fs::read_to_string(&plan_path).unwrap();
    assert_eq!(
        unbounded_text.lines().count(),
        2,
        "without --budget, --plan should write every entry"
    );

    std::fs::remove_file(&plan_path).ok();
}

#[test]
fn stable_wrong_risk_breakdown_off_by_default() {
    let path = gold_labeled_fixture();
    let out = run(&["stable-wrong-risk", path.to_str().unwrap()]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains("by_evaluator"));
    assert!(!text.contains("by_model"));
    assert!(!text.contains("by_budget"));
}

#[test]
fn stable_wrong_risk_breakdown_computes_real_evaluator_rate() {
    // "right": unanimous "win", gold "win", evaluator e1 -> Keep, matches gold.
    // "wrong": unanimous "loss", gold "win", evaluator e1 -> Keep, disagrees with gold.
    // e1 touches both Keep samples -> n_keep=2, n_stable_wrong=1, rate=0.5.
    let path = write_temp(
        "stable_wrong_breakdown.jsonl",
        "{\"sample_id\":\"right\",\"label\":\"win\",\"gold_label\":\"win\",\"evaluator_id\":\"e1\"}\n\
         {\"sample_id\":\"right\",\"label\":\"win\",\"gold_label\":\"win\",\"evaluator_id\":\"e1\"}\n\
         {\"sample_id\":\"right\",\"label\":\"win\",\"gold_label\":\"win\",\"evaluator_id\":\"e1\"}\n\
         {\"sample_id\":\"wrong\",\"label\":\"loss\",\"gold_label\":\"win\",\"evaluator_id\":\"e1\"}\n\
         {\"sample_id\":\"wrong\",\"label\":\"loss\",\"gold_label\":\"win\",\"evaluator_id\":\"e1\"}\n\
         {\"sample_id\":\"wrong\",\"label\":\"loss\",\"gold_label\":\"win\",\"evaluator_id\":\"e1\"}\n",
    );
    let out = run(&["stable-wrong-risk", path.to_str().unwrap(), "--breakdown"]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let by_evaluator = value["by_evaluator"].as_array().unwrap();
    assert_eq!(by_evaluator.len(), 1);
    assert_eq!(by_evaluator[0]["evaluator_id"], "e1");
    assert_eq!(by_evaluator[0]["n_keep"], 2);
    assert_eq!(by_evaluator[0]["n_stable_wrong"], 1);
    assert!((by_evaluator[0]["stable_wrong_rate"].as_f64().unwrap() - 0.5).abs() < 1e-9);
}
