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
