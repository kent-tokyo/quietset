// Lightweight guard against README/docs drift: no markdown parsing, just substring checks
// on whitespace-normalized file content. English soft-wraps at spaces (safe to fold into a
// single space); Japanese has no inter-word spaces, so any whitespace/newline is stripped
// instead — otherwise a wrapped source line would silently break a substring match here the
// same way it silently breaks CJK rendering (see tasks/lessons.md).

use std::fs;

const METRIC_IDS: &[&str] = &[
    "label_agreement_lcb",
    "fleiss_kappa",
    "krippendorff_alpha",
    "label_entropy",
    "score_std",
    "score_mad",
    "score_iqr",
    "latent_truth_",
    "stable_wrong_rate_among_keep",
    "stability_score",
    "confidence",
    "adjusted_stability_score",
    "score_sign_agreement",
];

fn read(relative_path: &str) -> String {
    fs::read_to_string(relative_path)
        .unwrap_or_else(|e| panic!("failed to read {relative_path}: {e}"))
}

fn normalize_en(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_ja(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn section(content: &str, heading: &str) -> String {
    let start = content
        .find(heading)
        .unwrap_or_else(|| panic!("heading {heading:?} not found"));
    let rest = &content[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn readme_provenance_table_and_metrics_doc_list_the_same_metrics() {
    let readme_en = normalize_en(&section(
        &read("../../README.md"),
        "## Where these numbers come from",
    ));
    let metrics_en = normalize_en(&read("../../docs/metrics.md"));
    let readme_ja = normalize_ja(&section(
        &read("../../README_ja.md"),
        "## これらの数値の根拠",
    ));
    let metrics_ja = normalize_ja(&read("../../docs/metrics_ja.md"));

    for id in METRIC_IDS {
        assert!(
            readme_en.contains(id),
            "{id} missing from README.md's provenance table"
        );
        assert!(metrics_en.contains(id), "{id} missing from docs/metrics.md");
        assert!(
            readme_ja.contains(id),
            "{id} missing from README_ja.md's provenance table"
        );
        assert!(
            metrics_ja.contains(id),
            "{id} missing from docs/metrics_ja.md"
        );
    }
}

// Each doc independently phrases these caveats in its own words (not a translation-identical
// pair), so the expected phrase is verified per file rather than assumed shared. The point of
// the test is to catch a future edit that silently drops the caveat from one file, not to
// enforce identical wording across languages/files.

#[test]
fn stability_score_is_documented_as_not_a_calibrated_probability() {
    assert!(normalize_en(&read("../../README.md")).contains("not a calibrated probability"));
    assert!(normalize_en(&read("../../docs/metrics.md")).contains("not a calibrated probability"));
    assert!(
        normalize_ja(&read("../../README_ja.md")).contains(&normalize_ja(
            "キャリブレーションされた確率でも正しさの保証でもありません"
        ))
    );
    assert!(
        normalize_ja(&read("../../docs/metrics_ja.md")).contains(&normalize_ja(
            "キャリブレーションされた確率ではなく、正しさの保証でもない"
        ))
    );
}

#[test]
fn latent_truth_confidence_caveat_is_documented_as_not_a_correctness_guarantee() {
    assert!(normalize_en(&read("../../README.md")).contains("not a correctness guarantee"));
    assert!(
        normalize_en(&read("../../docs/metrics.md")).contains("not an external correctness check")
    );
    assert!(
        normalize_ja(&read("../../README_ja.md")).contains(&normalize_ja("正しさの保証ではない"))
    );
    assert!(
        normalize_ja(&read("../../docs/metrics_ja.md"))
            .contains(&normalize_ja("外部からの正しさのチェックではない"))
    );
}
