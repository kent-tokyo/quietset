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
fn version_flag_reports_cargo_package_version() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn score_jsonl_succeeds_on_valid_fixture() {
    let out = run(&["score", "../../tests/fixtures/simple.jsonl"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("\"sample_id\":\"a\""));
}

// simple.jsonl: sample "a" is stable (Keep), sample "b" is not -- so the default summary
// should report kept 1/2, review 1, drop 0, on stderr, leaving stdout as pure JSONL.
#[test]
fn score_prints_keep_rate_summary_to_stderr_by_default() {
    let out = run(&["score", "../../tests/fixtures/simple.jsonl"]);
    assert!(out.status.success());
    assert!(stderr(&out).contains("kept 1 / 2"));
    assert!(stderr(&out).contains("review 1"));
    assert!(stderr(&out).contains("drop 0"));
    for line in stdout(&out).lines() {
        assert!(
            serde_json::from_str::<serde_json::Value>(line).is_ok(),
            "summary output must stay on stderr, not leak into stdout JSONL: {line}"
        );
    }
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

#[test]
fn game_ai_profile_keeps_stable_signed_scores() {
    let path = write_temp(
        "game_ai_stable_signed.jsonl",
        "{\"sample_id\":\"stable_signed\",\"budget\":10.0,\"seed\":1,\"evaluator_id\":\"e1\",\"score\":0.35}\n\
         {\"sample_id\":\"stable_signed\",\"budget\":20.0,\"seed\":2,\"evaluator_id\":\"e2\",\"score\":0.52}\n\
         {\"sample_id\":\"stable_signed\",\"budget\":10.0,\"seed\":2,\"evaluator_id\":\"e1\",\"score\":0.41}\n\
         {\"sample_id\":\"stable_signed\",\"budget\":20.0,\"seed\":1,\"evaluator_id\":\"e2\",\"score\":0.44}\n",
    );
    let out = run(&["score", path.to_str().unwrap(), "--profile", "game-ai"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_eq!(value["decision"], "keep");
}

// game-ai weights score_sign at 2.0 because a shogi-style evaluation whose sign flips
// under small search perturbations is a worse training signal than one whose magnitude
// merely varies. Same budget/seed/evaluator spread as stable_signed above -- only the
// score-sign pattern differs.
#[test]
fn game_ai_profile_flags_sign_flip_instability() {
    let path = write_temp(
        "game_ai_sign_flip.jsonl",
        "{\"sample_id\":\"sign_flip\",\"budget\":10.0,\"seed\":1,\"evaluator_id\":\"e1\",\"score\":0.12}\n\
         {\"sample_id\":\"sign_flip\",\"budget\":20.0,\"seed\":2,\"evaluator_id\":\"e2\",\"score\":-0.08}\n\
         {\"sample_id\":\"sign_flip\",\"budget\":10.0,\"seed\":2,\"evaluator_id\":\"e1\",\"score\":0.05}\n\
         {\"sample_id\":\"sign_flip\",\"budget\":20.0,\"seed\":1,\"evaluator_id\":\"e2\",\"score\":-0.03}\n",
    );
    let out = run(&["score", path.to_str().unwrap(), "--profile", "game-ai"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_ne!(value["decision"], "keep");
}

// budget swing correlates with the sign flip while seed is assigned orthogonally to it,
// isolating budget_stability as the dominant weighted deficit rather than seed noise.
#[test]
fn game_ai_profile_flags_budget_flip_instability() {
    let path = write_temp(
        "game_ai_budget_flip.jsonl",
        "{\"sample_id\":\"budget_flip\",\"budget\":10.0,\"seed\":1,\"evaluator_id\":\"e1\",\"score\":0.40}\n\
         {\"sample_id\":\"budget_flip\",\"budget\":10.0,\"seed\":2,\"evaluator_id\":\"e2\",\"score\":0.38}\n\
         {\"sample_id\":\"budget_flip\",\"budget\":100.0,\"seed\":1,\"evaluator_id\":\"e2\",\"score\":-0.42}\n\
         {\"sample_id\":\"budget_flip\",\"budget\":100.0,\"seed\":2,\"evaluator_id\":\"e1\",\"score\":-0.36}\n",
    );
    let out = run(&["score", path.to_str().unwrap(), "--profile", "game-ai"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_ne!(value["decision"], "keep");
}

// Same seed-alternating pattern as active_review_observations_populates_target_seed_for_add_seed_action
// below, with a fixed budget and two evaluators added so it also clears game-ai's other
// minimums -- confirms seed instability alone demotes a sample under this stricter profile,
// and that the resulting active-review recommendation still targets add_seed.
#[test]
fn game_ai_profile_flags_seed_sensitivity_and_targets_add_seed() {
    let obs_text = "{\"sample_id\":\"pricey\",\"label\":\"win\",\"budget\":4.0,\"seed\":0,\"evaluator_id\":\"e1\",\"score\":0.9}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"budget\":4.0,\"seed\":1,\"evaluator_id\":\"e2\",\"score\":0.1}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"budget\":4.0,\"seed\":0,\"evaluator_id\":\"e2\",\"score\":0.9}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"budget\":4.0,\"seed\":1,\"evaluator_id\":\"e1\",\"score\":0.1}\n";
    let obs_path = write_temp("game_ai_seed_sensitive.jsonl", obs_text);

    let scored = run(&["score", obs_path.to_str().unwrap(), "--profile", "game-ai"]);
    assert!(scored.status.success());
    let scored_text = stdout(&scored);
    let value: serde_json::Value =
        serde_json::from_str(scored_text.lines().next().unwrap()).unwrap();
    assert_ne!(value["decision"], "keep");

    let scored_path = write_temp("game_ai_seed_sensitive_scored.jsonl", &scored_text);
    let out = run(&[
        "active-review",
        scored_path.to_str().unwrap(),
        "--observations",
        obs_path.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let review_value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_eq!(review_value["suggested_action"], "add_seed");
    assert_eq!(review_value["target_seed"], 2);
}

// Single-engine, multi-depth data: one distinct evaluator_id. --profile game-ai's
// min-evaluators-keep floor of 2 can never be satisfied by this shape, silently
// demoting an otherwise-stable sample to review; --profile game-ai-single-engine
// drops that floor to 0 since depth (budget), not evaluator identity, is what varies.
#[test]
fn game_ai_single_engine_profile_keeps_what_game_ai_profile_wrongly_demotes() {
    let path = write_temp(
        "game_ai_single_engine.jsonl",
        "{\"sample_id\":\"one_engine\",\"budget\":10.0,\"seed\":1,\"evaluator_id\":\"sekirei\",\"score\":0.35}\n\
         {\"sample_id\":\"one_engine\",\"budget\":20.0,\"seed\":2,\"evaluator_id\":\"sekirei\",\"score\":0.52}\n\
         {\"sample_id\":\"one_engine\",\"budget\":10.0,\"seed\":2,\"evaluator_id\":\"sekirei\",\"score\":0.41}\n\
         {\"sample_id\":\"one_engine\",\"budget\":20.0,\"seed\":1,\"evaluator_id\":\"sekirei\",\"score\":0.44}\n",
    );

    let under_game_ai = run(&["score", path.to_str().unwrap(), "--profile", "game-ai"]);
    assert!(under_game_ai.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&under_game_ai).lines().next().unwrap()).unwrap();
    assert_ne!(value["decision"], "keep");

    let under_single_engine = run(&[
        "score",
        path.to_str().unwrap(),
        "--profile",
        "game-ai-single-engine",
    ]);
    assert!(under_single_engine.status.success());
    let value_single: serde_json::Value =
        serde_json::from_str(stdout(&under_single_engine).lines().next().unwrap()).unwrap();
    assert_eq!(value_single["decision"], "keep");
}

// Same single-evaluator shape as the test above, but checking the proactive stderr warning
// rather than the decision itself: --profile game-ai's min-evaluators-keep floor of 2 can
// never be met by one evaluator_id, so this must warn and point at the single-engine profile;
// --profile game-ai-single-engine sets no evaluator floor, so it must stay silent.
#[test]
fn score_warns_when_min_evaluators_keep_exceeds_dataset_evaluator_count() {
    let path = write_temp(
        "min_evaluators_warning.jsonl",
        "{\"sample_id\":\"one_engine\",\"budget\":10.0,\"seed\":1,\"evaluator_id\":\"sekirei\",\"score\":0.35}\n\
         {\"sample_id\":\"one_engine\",\"budget\":20.0,\"seed\":2,\"evaluator_id\":\"sekirei\",\"score\":0.52}\n\
         {\"sample_id\":\"one_engine\",\"budget\":10.0,\"seed\":2,\"evaluator_id\":\"sekirei\",\"score\":0.41}\n\
         {\"sample_id\":\"one_engine\",\"budget\":20.0,\"seed\":1,\"evaluator_id\":\"sekirei\",\"score\":0.44}\n",
    );

    let under_game_ai = run(&["score", path.to_str().unwrap(), "--profile", "game-ai"]);
    assert!(under_game_ai.status.success());
    assert!(stderr(&under_game_ai).contains("game-ai-single-engine"));

    let under_single_engine = run(&[
        "score",
        path.to_str().unwrap(),
        "--profile",
        "game-ai-single-engine",
    ]);
    assert!(under_single_engine.status.success());
    assert!(!stderr(&under_single_engine).contains("game-ai-single-engine"));
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
fn active_review_observations_populates_target_seed_for_add_seed_action() {
    // seed alternates 0/1 with score flipping 0.9/0.1 -> high seed_sensitivity dominates,
    // so suggested_action is deterministically "add_seed"; max observed seed is 1.
    let obs_text = "{\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":0,\"score\":0.9}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":1,\"score\":0.1}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":0,\"score\":0.9}\n\
         {\"sample_id\":\"pricey\",\"label\":\"win\",\"seed\":1,\"score\":0.1}\n";
    let obs_path = write_temp("active_review_targets_obs.jsonl", obs_text);

    let scored = run(&["score", obs_path.to_str().unwrap()]);
    assert!(scored.status.success());
    let scored_path = write_temp("active_review_targets_scored.jsonl", &stdout(&scored));

    let out = run(&["active-review", scored_path.to_str().unwrap()]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_eq!(value["suggested_action"], "add_seed");
    assert!(
        value.get("target_seed").is_none(),
        "target_seed should be absent without --observations"
    );

    let out_with_obs = run(&[
        "active-review",
        scored_path.to_str().unwrap(),
        "--observations",
        obs_path.to_str().unwrap(),
    ]);
    assert!(out_with_obs.status.success());
    let value_with_obs: serde_json::Value =
        serde_json::from_str(stdout(&out_with_obs).lines().next().unwrap()).unwrap();
    assert_eq!(
        value_with_obs["target_seed"], 2,
        "target_seed should be max observed seed (1) + 1"
    );
    assert!(value_with_obs.get("target_budget").is_none());
    assert!(value_with_obs.get("target_evaluator_slot").is_none());
    assert!(value_with_obs.get("target_model_slot").is_none());
}

#[test]
fn active_review_observations_populates_target_budget_for_increase_budget_action() {
    // budget swing (0.1 at budget 2 vs 0.9 at budget 4) makes budget_sensitivity the
    // dominant signal over the unavoidable competing score_mad signal.
    let obs_text = "{\"sample_id\":\"s_budget\",\"score\":0.1,\"budget\":2.0}\n\
         {\"sample_id\":\"s_budget\",\"score\":0.9,\"budget\":4.0}\n";
    let obs_path = write_temp("active_review_targets_budget_obs.jsonl", obs_text);

    let scored = run(&["score", obs_path.to_str().unwrap()]);
    assert!(scored.status.success());
    let scored_path = write_temp(
        "active_review_targets_budget_scored.jsonl",
        &stdout(&scored),
    );

    let out = run(&[
        "active-review",
        scored_path.to_str().unwrap(),
        "--observations",
        obs_path.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_eq!(value["suggested_action"], "increase_budget");
    assert_eq!(
        value["target_budget"], 8.0,
        "target_budget should be max observed budget (4.0) x 2"
    );
}

#[test]
fn active_review_observations_populates_target_evaluator_slot_for_add_evaluator_action() {
    // 50/50 label split across two evaluators maximizes label_entropy, which dominates
    // over label_agreement_lcb here.
    let obs_text = "{\"sample_id\":\"s_evaluator\",\"label\":\"win\",\"evaluator_id\":\"e1\"}\n\
         {\"sample_id\":\"s_evaluator\",\"label\":\"win\",\"evaluator_id\":\"e1\"}\n\
         {\"sample_id\":\"s_evaluator\",\"label\":\"loss\",\"evaluator_id\":\"e2\"}\n\
         {\"sample_id\":\"s_evaluator\",\"label\":\"loss\",\"evaluator_id\":\"e2\"}\n";
    let obs_path = write_temp("active_review_targets_evaluator_obs.jsonl", obs_text);

    let scored = run(&["score", obs_path.to_str().unwrap()]);
    assert!(scored.status.success());
    let scored_path = write_temp(
        "active_review_targets_evaluator_scored.jsonl",
        &stdout(&scored),
    );

    let out = run(&[
        "active-review",
        scored_path.to_str().unwrap(),
        "--observations",
        obs_path.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_eq!(value["suggested_action"], "add_evaluator");
    assert_eq!(
        value["target_evaluator_slot"], 3,
        "target_evaluator_slot should be distinct evaluator count (2) + 1"
    );
}

#[test]
fn active_review_observations_populates_target_model_slot_for_add_model_action() {
    // no label/budget/seed fields at all, so score_mad is the only live signal.
    let obs_text = "{\"sample_id\":\"s_model\",\"score\":0.1,\"model_id\":\"m1\"}\n\
         {\"sample_id\":\"s_model\",\"score\":0.9,\"model_id\":\"m2\"}\n";
    let obs_path = write_temp("active_review_targets_model_obs.jsonl", obs_text);

    let scored = run(&["score", obs_path.to_str().unwrap()]);
    assert!(scored.status.success());
    let scored_path = write_temp("active_review_targets_model_scored.jsonl", &stdout(&scored));

    let out = run(&[
        "active-review",
        scored_path.to_str().unwrap(),
        "--observations",
        obs_path.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout(&out).lines().next().unwrap()).unwrap();
    assert_eq!(value["suggested_action"], "add_model");
    assert_eq!(
        value["target_model_slot"], 3,
        "target_model_slot should be distinct model count (2) + 1"
    );
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

#[test]
fn block_score_classifies_a_stable_growth_block() {
    let path = write_temp(
        "block_score_growth.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B1\",\"seed\":1,\"model_id\":\"ckpt1\",\"trajectory_effect\":0.3,\"update_cosine\":0.9}\n\
         {\"sample_id\":\"p1\",\"block_id\":\"B1\",\"seed\":2,\"model_id\":\"ckpt2\",\"trajectory_effect\":0.4,\"update_cosine\":0.8}\n",
    );
    let out = run(&["block-score", path.to_str().unwrap()]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(value["block_id"], "B1");
    assert_eq!(value["classification"], "stable_growth");
}

#[test]
fn block_score_warns_when_no_block_id_present() {
    let path = write_temp(
        "block_score_no_block.jsonl",
        "{\"sample_id\":\"p1\",\"score\":0.9}\n",
    );
    let out = run(&["block-score", path.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(stdout(&out).trim().is_empty());
    assert!(stderr(&out).contains("no observation carried block_id"));
}

#[test]
fn trajectory_audit_flags_a_destructive_block_like_b5() {
    let before = write_temp(
        "trajectory_audit_before.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B5\",\"seed\":1,\"layer_id\":\"ft\",\"dead_unit_count\":0,\"trajectory_effect\":-0.1}\n\
         {\"sample_id\":\"p2\",\"block_id\":\"B5\",\"seed\":2,\"layer_id\":\"ft\",\"dead_unit_count\":0,\"trajectory_effect\":-0.1}\n",
    );
    let after = write_temp(
        "trajectory_audit_after.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B5\",\"seed\":1,\"layer_id\":\"ft\",\"dead_unit_count\":6,\"trajectory_effect\":-0.5}\n\
         {\"sample_id\":\"p2\",\"block_id\":\"B5\",\"seed\":2,\"layer_id\":\"ft\",\"dead_unit_count\":5,\"trajectory_effect\":-0.6}\n",
    );
    let out = run(&[
        "trajectory-audit",
        "--before",
        before.to_str().unwrap(),
        "--after",
        after.to_str().unwrap(),
        "--group-size",
        "32",
        "--json",
    ]);
    assert!(out.status.success());
    assert!(
        stderr(&out).contains("expected 32"),
        "should warn about the 2-sample block not matching --group-size 32: {}",
        stderr(&out)
    );
    let value: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(value["block_id"], "B5");
    assert_eq!(value["before_classification"], "stable_shrink");
    assert_eq!(value["after_classification"], "pathological");
    assert_eq!(value["dead_unit_delta"]["ft"], 1.0);
}

#[test]
fn trajectory_audit_reports_nothing_when_no_blocks_match() {
    let before = write_temp(
        "trajectory_audit_nomatch_before.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B5\"}\n",
    );
    let after = write_temp(
        "trajectory_audit_nomatch_after.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B7\"}\n",
    );
    let out = run(&[
        "trajectory-audit",
        "--before",
        before.to_str().unwrap(),
        "--after",
        after.to_str().unwrap(),
    ]);
    assert!(out.status.success());
    assert!(stdout(&out).trim().is_empty());
    assert!(stderr(&out).contains("nothing to report"));
}

#[test]
fn block_score_warns_and_marks_insufficient_on_mixed_loss_recipe() {
    let path = write_temp(
        "block_score_mixed_recipe.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B5\",\"seed\":1,\"trajectory_effect\":0.3,\"loss_recipe\":\"recipe_a\"}\n\
         {\"sample_id\":\"p2\",\"block_id\":\"B5\",\"seed\":2,\"trajectory_effect\":0.4,\"loss_recipe\":\"recipe_b\"}\n",
    );
    let out = run(&["block-score", path.to_str().unwrap()]);
    assert!(out.status.success());
    assert!(
        stderr(&out).contains("mixes loss_recipe values"),
        "{}",
        stderr(&out)
    );
    let value: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(value["classification"], "insufficient");
}

#[test]
fn trajectory_audit_marks_loss_recipe_mismatch_not_comparable() {
    let before = write_temp(
        "trajectory_audit_recipe_before.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B5\",\"loss_recipe\":\"baseline\",\"dead_unit_count\":0}\n",
    );
    let after = write_temp(
        "trajectory_audit_recipe_after.jsonl",
        "{\"sample_id\":\"p1\",\"block_id\":\"B5\",\"loss_recipe\":\"teacher_conflict_masking\",\"dead_unit_count\":5}\n",
    );
    let out = run(&[
        "trajectory-audit",
        "--before",
        before.to_str().unwrap(),
        "--after",
        after.to_str().unwrap(),
        "--json",
    ]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(value["comparable"], false);
    assert_eq!(value["comparison_issue"], "loss_recipe_mismatch");
    assert!(value["dead_unit_delta"].as_object().unwrap().is_empty());
}

// Regression guard for the trajectory-stability feature set: `score --output-format csv` is a
// hand-written fixed-column writer that was never updated to include the 5 new components, so
// its header must be byte-for-byte identical to what it was before this feature existed. If a
// future change adds trajectory columns to CSV, this test should be updated deliberately, not
// silently pass with an unexpectedly wider header.
#[test]
fn score_csv_header_is_unchanged_by_trajectory_stability_fields() {
    let out = run(&[
        "score",
        "../../tests/fixtures/simple.jsonl",
        "--output-format",
        "csv",
    ]);
    assert!(out.status.success());
    let header = stdout(&out).lines().next().unwrap().to_string();
    assert_eq!(
        header,
        "sample_id,n_observations,majority_label,label_agreement,label_agreement_lcb,\
         label_margin,label_entropy,score_mean,score_std,score_range,score_mad,score_iqr,\
         budget_sensitivity,budget_slope,seed_sensitivity,model_agreement,evaluator_agreement,\
         confidence,adjusted_stability_score,disagreement_score,stability_score,decision,\
         component_label,component_score_consistency,component_budget_robustness,\
         component_seed_robustness,component_model_agreement,component_evaluator_agreement"
    );
}
