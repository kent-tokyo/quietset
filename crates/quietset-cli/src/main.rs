use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use std::cmp::Reverse;
use std::collections::HashMap;

use quietset::{
    Decision, DecisionScore, MinRequirements, Observation, ScoreConfig, ScoreDispersion,
    ScoreWeights, StabilityReport, Thresholds, compute_calibration, compute_evaluator_reliability,
    compute_fleiss_kappa, compute_krippendorff_alpha, parse_csv, parse_jsonl, score_all,
    score_all_weighted,
};

#[derive(Parser)]
#[command(name = "quietset", about = "Filter datasets by label stability")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Score observations and output StabilityReports
    Score(ScoreArgs),
    /// Filter scored JSONL by stability/decision thresholds
    Filter(FilterArgs),
    /// Print aggregate statistics for a scored JSONL file
    Summary(SummaryArgs),
    /// Print a detailed breakdown for one sample from a scored JSONL file
    Explain(ExplainArgs),
    /// Compare two scored JSONL files by sample_id
    Compare(CompareArgs),
    /// Estimate per-evaluator reliability from an observation JSONL file (experimental)
    Reliability(ReliabilityArgs),
    /// Deep diagnostic report for a scored JSONL file
    Audit(AuditArgs),
    /// Extract samples by diagnostic class for human review queues
    Select(SelectArgs),
    /// Suggest which samples to re-evaluate and why
    Recommend(RecommendArgs),
    /// Compute the fraction of kept samples that are stably wrong (requires gold_label)
    StableWrongRisk(StableWrongRiskArgs),
    /// Find a keep_threshold matching a precision or coverage target using gold labels
    Calibrate(CalibrateArgs),
    /// Sweep keep_threshold values and show the precision/coverage trade-off table
    Policy(PolicyArgs),
    /// Rank scored samples by re-evaluation urgency
    ActiveReview(ActiveReviewArgs),
}

#[derive(ValueEnum, Clone)]
enum DecisionScoreArg {
    Raw,
    Adjusted,
    Lcb,
}

#[derive(ValueEnum, Clone)]
enum ProfileArg {
    /// LLM judge: weight evaluator and model agreement 2×; default decision-score lcb.
    LlmJudge,
    /// Simulation: weight budget and seed robustness 2×; default decision-score adjusted.
    Simulation,
    /// Game/search AI: weight budget 2×, seed 2×; default decision-score lcb; min-observations 4, min-budgets 2, min-seeds 2.
    GameAi,
    /// Benchmark curation: weight label 2×, evaluator 1.5×; default decision-score raw.
    Benchmark,
}

#[derive(ValueEnum, Clone)]
enum VoteArg {
    /// Standard majority vote (default).
    Raw,
    /// Reliability-weighted vote using per-evaluator accuracy (2-pass).
    Weighted,
}

#[derive(ValueEnum, Clone)]
enum DispersionArg {
    /// Standard deviation (default, backward-compatible).
    Std,
    /// Median absolute deviation — more robust to occasional outlier scores.
    Mad,
    /// Interquartile range — more robust to occasional outlier scores.
    Iqr,
}

#[derive(clap::Args)]
struct ScoreArgs {
    /// Input file (JSONL or CSV). Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,

    /// Input format.
    #[arg(long, default_value = "jsonl", value_enum)]
    format: Format,

    /// Output format.
    #[arg(long, default_value = "jsonl", value_enum)]
    output_format: Format,

    /// Output file. Default: stdout.
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// Scale for normalizing score_std and sensitivity metrics (must be > 0).
    #[arg(long, default_value_t = 1.0)]
    score_scale: f64,

    /// Stability threshold for 'keep' decision.
    #[arg(long, default_value_t = 0.85)]
    keep_threshold: f64,

    /// Stability threshold for 'drop' decision.
    #[arg(long, default_value_t = 0.40)]
    drop_threshold: f64,

    /// Skip malformed lines instead of exiting with an error.
    #[arg(long)]
    skip_invalid: bool,

    /// Confidence smoothing parameter: confidence = n / (n + k). Default 3.0.
    #[arg(long, default_value_t = 3.0)]
    confidence_k: f64,

    /// Decision score mode for keep/review/drop: raw (default), adjusted, or lcb.
    /// Overrides --use-adjusted-score and --use-lcb-score when specified.
    #[arg(long, value_enum)]
    decision_score: Option<DecisionScoreArg>,

    /// Alias for --decision-score adjusted.
    #[arg(long)]
    use_adjusted_score: bool,

    /// Alias for --decision-score lcb.
    #[arg(long)]
    use_lcb_score: bool,

    /// Confidence level for Wilson LCB. Default 0.95.
    #[arg(long, default_value_t = 0.95)]
    confidence_level: f64,

    /// Minimum total observations required for Keep (lower-evidence samples demoted to Review).
    #[arg(long, default_value_t = 1)]
    min_observations_keep: usize,

    /// Minimum distinct evaluator_ids required for Keep.
    #[arg(long, default_value_t = 0)]
    min_evaluators_keep: usize,

    /// Minimum distinct seeds required for Keep.
    #[arg(long, default_value_t = 0)]
    min_seeds_keep: usize,

    /// Minimum distinct budget levels required for Keep.
    #[arg(long, default_value_t = 0)]
    min_budgets_keep: usize,

    /// Minimum distinct model_ids required for Keep.
    #[arg(long, default_value_t = 0)]
    min_models_keep: usize,

    /// Print per-evaluator reliability estimates to stderr after scoring (experimental).
    #[arg(long)]
    estimate_evaluator_reliability: bool,

    /// Append a trailing dataset stats line (fleiss_kappa, krippendorff_alpha) to the scored
    /// output so that `audit --json` can include them without a separate --observations flag.
    #[arg(long)]
    embed_stats: bool,

    /// Weight for label_agreement in stability_score (0 = exclude).
    #[arg(long)]
    weight_labels: Option<f64>,

    /// Weight for score stability (1 - normalized_score_std) in stability_score.
    #[arg(long)]
    weight_scores: Option<f64>,

    /// Weight for budget stability (1 - budget_sensitivity) in stability_score.
    #[arg(long)]
    weight_budget: Option<f64>,

    /// Weight for seed stability (1 - seed_sensitivity) in stability_score.
    #[arg(long)]
    weight_seed: Option<f64>,

    /// Weight for model_agreement in stability_score.
    #[arg(long)]
    weight_models: Option<f64>,

    /// Weight for evaluator_agreement in stability_score.
    #[arg(long)]
    weight_evaluators: Option<f64>,

    /// Apply a use-case preset. Explicit --weight-* and --decision-score flags override the preset.
    #[arg(long, value_enum)]
    profile: Option<ProfileArg>,

    /// Vote aggregation mode: raw (default) or weighted (uses per-evaluator reliability, 2-pass).
    #[arg(long, default_value = "raw", value_enum)]
    vote: VoteArg,

    /// Dispersion metric for score_consistency: std (default), mad, or iqr.
    /// mad/iqr are more robust when occasional outlier scores are present.
    #[arg(long, default_value = "std", value_enum)]
    score_dispersion: DispersionArg,
}

#[derive(clap::Args)]
struct SummaryArgs {
    /// Input scored JSONL file. Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,

    /// Skip malformed lines instead of exiting with an error.
    #[arg(long)]
    skip_invalid: bool,

    /// Output machine-readable JSON instead of formatted text.
    #[arg(long)]
    json: bool,

    /// Keep threshold used during scoring (for LCB demotion analysis). Default 0.85.
    #[arg(long, default_value_t = 0.85)]
    keep_threshold: f64,
}

#[derive(clap::Args)]
struct ExplainArgs {
    /// Input scored JSONL file. Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,

    /// The sample_id to explain.
    #[arg(long)]
    sample_id: String,

    /// Output the raw StabilityReport JSON instead of formatted text.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct CompareArgs {
    /// Scored JSONL file from the baseline run.
    before: String,

    /// Scored JSONL file from the new run.
    after: String,

    /// Output machine-readable JSON instead of formatted text.
    #[arg(long)]
    json: bool,

    /// Number of top regressions to display.
    #[arg(long, default_value_t = 5)]
    top: usize,

    /// Show per-component mean deltas.
    #[arg(long)]
    components: bool,

    /// Compare after-file decisions against a hypothetical decision-score policy.
    #[arg(long, value_enum)]
    policy_after: Option<DecisionScoreArg>,

    /// Keep threshold for policy comparison (default 0.85).
    #[arg(long, default_value_t = 0.85)]
    policy_keep_threshold: f64,

    /// Drop threshold for policy comparison (default 0.40).
    #[arg(long, default_value_t = 0.40)]
    policy_drop_threshold: f64,
}

#[derive(clap::Args)]
struct FilterArgs {
    /// Input scored JSONL file. Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,

    /// Output file. Default: stdout.
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// Keep only records with stability_score >= this value.
    #[arg(long)]
    min_stability: Option<f64>,

    /// Keep only records with disagreement_score <= this value.
    #[arg(long)]
    max_disagreement: Option<f64>,

    /// Keep only records with this decision.
    #[arg(long, value_enum)]
    decision: Option<DecisionArg>,

    /// Skip malformed lines instead of exiting with an error.
    #[arg(long)]
    skip_invalid: bool,

    /// Keep only records with label_agreement_lcb >= this value.
    #[arg(long)]
    min_label_lcb: Option<f64>,

    /// Keep only records with confidence >= this value.
    #[arg(long)]
    min_confidence: Option<f64>,

    /// Keep only records with score_mad <= this value.
    #[arg(long)]
    max_score_mad: Option<f64>,

    /// Keep only records with score_iqr <= this value.
    #[arg(long)]
    max_score_iqr: Option<f64>,
}

#[derive(clap::Args)]
struct ReliabilityArgs {
    /// Input observation JSONL file (not scored). Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,

    /// Skip malformed lines instead of exiting with an error.
    #[arg(long)]
    skip_invalid: bool,
}

#[derive(clap::Args)]
struct AuditArgs {
    #[arg(default_value = "-")]
    input: String,
    #[arg(long)]
    json: bool,
    #[arg(long, default_value_t = 10)]
    top: usize,
    #[arg(long)]
    skip_invalid: bool,
    #[arg(long, default_value_t = 0.85)]
    keep_threshold: f64,
    /// Optional observation JSONL for dataset-level agreement stats (Fleiss kappa, Krippendorff alpha).
    #[arg(long)]
    observations: Option<String>,
}

#[derive(ValueEnum, Clone)]
enum SelectClass {
    /// Samples in the uncertainty band (0.75 <= stability <= 0.95).
    Borderline,
    /// Samples sorted by disagreement_score descending.
    HighDisagreement,
    /// Samples sorted by budget_sensitivity descending.
    BudgetSensitive,
    /// Samples sorted by seed_sensitivity descending.
    SeedSensitive,
    /// Samples with stability >= keep_threshold but label_agreement_lcb < keep_threshold.
    HighRawLowLcb,
    /// Samples sorted by score_mad descending.
    HighScoreMad,
}

#[derive(clap::Args)]
struct SelectArgs {
    /// Input scored JSONL file. Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,
    /// Class of samples to extract.
    #[arg(long, value_enum)]
    class: SelectClass,
    /// Maximum number of samples to output. Default: all matching samples.
    #[arg(long)]
    top: Option<usize>,
    /// Keep threshold (used by borderline and high-raw-low-lcb classes).
    #[arg(long, default_value_t = 0.85)]
    keep_threshold: f64,
    /// Skip malformed lines.
    #[arg(long)]
    skip_invalid: bool,
}

#[derive(clap::Args)]
struct RecommendArgs {
    /// Input scored JSONL file. Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,
    /// Skip malformed lines.
    #[arg(long)]
    skip_invalid: bool,
    /// Keep threshold (default 0.85).
    #[arg(long, default_value_t = 0.85)]
    keep_threshold: f64,
    /// Only emit recommendations for review or drop samples (skip keep with no LCB risk).
    #[arg(long)]
    unstable_only: bool,
    /// Human-readable text output instead of JSONL.
    #[arg(long)]
    text: bool,
}

#[derive(clap::Args)]
struct StableWrongRiskArgs {
    /// Input observation JSONL (must have gold_label). Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,
    /// Skip malformed lines.
    #[arg(long)]
    skip_invalid: bool,
    /// Keep threshold for scoring (default 0.85).
    #[arg(long, default_value_t = 0.85)]
    keep_threshold: f64,
    /// Number of stable-wrong samples to list (default 10).
    #[arg(long, default_value_t = 10)]
    top: usize,
}

#[derive(clap::Args)]
struct CalibrateArgs {
    #[arg(default_value = "-")]
    input: String,
    #[arg(long)]
    skip_invalid: bool,
    #[arg(long, default_value_t = 0.95)]
    target_precision: f64,
    #[arg(long)]
    target_coverage: Option<f64>,
    #[arg(long, value_enum)]
    decision_score: Option<DecisionScoreArg>,
    #[arg(long, default_value_t = 0.40)]
    drop_threshold: f64,
    #[arg(long, default_value_t = 0.95)]
    confidence_level: f64,
}

#[derive(clap::Args)]
struct PolicyArgs {
    /// Input observation JSONL (same format as `score`). Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,
    #[arg(long)]
    skip_invalid: bool,
    /// Decision score mode: raw (default), adjusted, or lcb.
    #[arg(long, value_enum)]
    decision_score: Option<DecisionScoreArg>,
    /// Confidence level for Wilson LCB. Default 0.95.
    #[arg(long, default_value_t = 0.95)]
    confidence_level: f64,
    /// Report the loosest threshold that meets this precision target.
    #[arg(long)]
    target_precision: Option<f64>,
    /// Report the loosest threshold that meets this coverage target.
    #[arg(long)]
    target_coverage: Option<f64>,
    /// Output machine-readable JSONL instead of a formatted table.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct ActiveReviewArgs {
    /// Input scored JSONL (output of `score`). Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,
    #[arg(long)]
    skip_invalid: bool,
    /// Only output review and drop samples (skip keep with no issues).
    #[arg(long)]
    unstable_only: bool,
    /// Maximum number of samples to output (default: all).
    #[arg(long)]
    top: Option<usize>,
    /// Weight for low-LCB signal (1 - label_agreement_lcb).
    #[arg(long, default_value_t = 1.0)]
    weight_lcb: f64,
    /// Weight for high-entropy signal.
    #[arg(long, default_value_t = 1.0)]
    weight_entropy: f64,
    /// Weight for high-score-mad signal.
    #[arg(long, default_value_t = 1.0)]
    weight_score_mad: f64,
    /// Weight for high-budget-sensitivity signal.
    #[arg(long, default_value_t = 1.0)]
    weight_budget_sensitivity: f64,
    /// Weight for high-seed-sensitivity signal.
    #[arg(long, default_value_t = 1.0)]
    weight_seed_sensitivity: f64,
}

#[derive(ValueEnum, Clone, Debug)]
enum Format {
    Jsonl,
    Csv,
}

#[derive(ValueEnum, Clone, Debug)]
enum DecisionArg {
    Keep,
    Review,
    Drop,
}

fn read_input(input: &str) -> Result<String> {
    if input == "-" {
        let stdin = io::stdin();
        let mut buf = String::new();
        for line in stdin.lock().lines() {
            buf.push_str(&line.context("reading stdin")?);
            buf.push('\n');
        }
        Ok(buf)
    } else {
        std::fs::read_to_string(input).with_context(|| format!("reading {input}"))
    }
}

fn open_output(path: Option<&PathBuf>) -> Result<Box<dyn Write>> {
    match path {
        Some(p) => Ok(Box::new(
            std::fs::File::create(p).with_context(|| format!("creating {}", p.display()))?,
        )),
        None => Ok(Box::new(io::stdout())),
    }
}

fn write_csv_reports<W: Write>(reports: &[StabilityReport], writer: W) -> Result<()> {
    let mut wtr = csv::Writer::from_writer(writer);
    wtr.write_record([
        "sample_id",
        "n_observations",
        "majority_label",
        "label_agreement",
        "label_agreement_lcb",
        "label_margin",
        "label_entropy",
        "score_mean",
        "score_std",
        "score_range",
        "score_mad",
        "score_iqr",
        "budget_sensitivity",
        "budget_slope",
        "seed_sensitivity",
        "model_agreement",
        "evaluator_agreement",
        "confidence",
        "adjusted_stability_score",
        "disagreement_score",
        "stability_score",
        "decision",
        "component_label",
        "component_score_consistency",
        "component_budget_robustness",
        "component_seed_robustness",
        "component_model_agreement",
        "component_evaluator_agreement",
    ])?;
    for r in reports {
        let c = &r.components;
        wtr.write_record([
            r.sample_id.as_str(),
            &r.n_observations.to_string(),
            r.majority_label.as_deref().unwrap_or(""),
            &r.label_agreement
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.label_agreement_lcb
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.label_margin
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.label_entropy
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.score_mean.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_std.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_range.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_mad.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_iqr.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.budget_sensitivity
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.budget_slope
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.seed_sensitivity
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.model_agreement
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.evaluator_agreement
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &format!("{:.6}", r.confidence),
            &format!("{:.6}", r.adjusted_stability_score),
            &format!("{:.6}", r.disagreement_score),
            &format!("{:.6}", r.stability_score),
            &r.decision.to_string(),
            &c.label.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &c.score_consistency
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &c.budget_robustness
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &c.seed_robustness
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &c.model_agreement
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &c.evaluator_agreement
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Score(args) => run_score(args),
        Commands::Filter(args) => run_filter(args),
        Commands::Summary(args) => run_summary(args),
        Commands::Explain(args) => run_explain(args),
        Commands::Compare(args) => run_compare(args),
        Commands::Reliability(args) => run_reliability(args),
        Commands::Audit(args) => run_audit(args),
        Commands::Select(args) => run_select(args),
        Commands::Recommend(args) => run_recommend(args),
        Commands::StableWrongRisk(args) => run_stable_wrong_risk(args),
        Commands::Calibrate(args) => run_calibrate(args),
        Commands::Policy(args) => run_policy(args),
        Commands::ActiveReview(args) => run_active_review(args),
    }
}

fn run_score(args: ScoreArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let observations = if args.skip_invalid {
        match args.format {
            Format::Jsonl => {
                let mut obs = Vec::new();
                for (i, line) in raw.lines().enumerate() {
                    let line = line.trim();
                    if line.is_empty() || line.contains("\"_quietset_stats\"") {
                        continue;
                    }
                    match serde_json::from_str::<Observation>(line) {
                        Ok(o) => match o.validate(i + 1) {
                            Ok(()) => obs.push(o),
                            Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
                        },
                        Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
                    }
                }
                obs
            }
            Format::Csv => {
                let mut obs = Vec::new();
                let mut rdr = csv::Reader::from_reader(raw.as_bytes());
                for (i, record) in rdr.deserialize::<Observation>().enumerate() {
                    match record {
                        Ok(o) => match o.validate(i + 1) {
                            Ok(()) => obs.push(o),
                            Err(e) => eprintln!("warning: skipping row {}: {e}", i + 1),
                        },
                        Err(e) => eprintln!("warning: skipping row {}: {e}", i + 1),
                    }
                }
                obs
            }
        }
    } else {
        match args.format {
            Format::Jsonl => parse_jsonl(&raw).context("parsing JSONL")?,
            Format::Csv => parse_csv(raw.as_bytes()).context("parsing CSV")?,
        }
    };
    if observations.is_empty() {
        anyhow::bail!("no observations found");
    }
    if args.decision_score.is_some() && (args.use_adjusted_score || args.use_lcb_score) {
        eprintln!("warning: --decision-score overrides --use-adjusted-score / --use-lcb-score");
    }
    let base_weights = match &args.profile {
        Some(ProfileArg::LlmJudge) => ScoreWeights {
            evaluator_agreement: 2.0,
            model_agreement: 2.0,
            ..ScoreWeights::default()
        },
        Some(ProfileArg::Simulation) => ScoreWeights {
            budget_stability: 2.0,
            seed_stability: 2.0,
            ..ScoreWeights::default()
        },
        Some(ProfileArg::GameAi) => ScoreWeights {
            budget_stability: 2.0,
            seed_stability: 2.0,
            ..ScoreWeights::default()
        },
        Some(ProfileArg::Benchmark) => ScoreWeights {
            label_agreement: 2.0,
            evaluator_agreement: 1.5,
            ..ScoreWeights::default()
        },
        None => ScoreWeights::default(),
    };
    let profile_decision: Option<DecisionScoreArg> = match &args.profile {
        Some(ProfileArg::LlmJudge) => Some(DecisionScoreArg::Lcb),
        Some(ProfileArg::Simulation) => Some(DecisionScoreArg::Adjusted),
        Some(ProfileArg::GameAi) => Some(DecisionScoreArg::Lcb),
        _ => None,
    };
    let is_game_ai = matches!(args.profile, Some(ProfileArg::GameAi));
    let profile_min_obs: usize = if is_game_ai { 4 } else { 0 };
    let profile_min_budgets: usize = if is_game_ai { 2 } else { 0 };
    let profile_min_seeds: usize = if is_game_ai { 2 } else { 0 };
    let config = ScoreConfig {
        score_scale: args.score_scale,
        thresholds: Thresholds {
            keep: args.keep_threshold,
            drop: args.drop_threshold,
        },
        weights: ScoreWeights {
            label_agreement: args.weight_labels.unwrap_or(base_weights.label_agreement),
            score_stability: args.weight_scores.unwrap_or(base_weights.score_stability),
            budget_stability: args.weight_budget.unwrap_or(base_weights.budget_stability),
            seed_stability: args.weight_seed.unwrap_or(base_weights.seed_stability),
            model_agreement: args.weight_models.unwrap_or(base_weights.model_agreement),
            evaluator_agreement: args
                .weight_evaluators
                .unwrap_or(base_weights.evaluator_agreement),
        },
        confidence_k: args.confidence_k,
        min_requirements: MinRequirements {
            observations: args.min_observations_keep.max(profile_min_obs),
            evaluators: args.min_evaluators_keep,
            seeds: args.min_seeds_keep.max(profile_min_seeds),
            budgets: args.min_budgets_keep.max(profile_min_budgets),
            models: args.min_models_keep,
        },
        // --decision-score wins; profile default next; --use-* are backwards-compat aliases (fallback only)
        decision_score: match (&args.decision_score, &profile_decision) {
            (Some(DecisionScoreArg::Lcb), _) => DecisionScore::LowerConfidenceBound,
            (Some(DecisionScoreArg::Adjusted), _) => DecisionScore::Adjusted,
            (Some(DecisionScoreArg::Raw), _) => DecisionScore::Raw,
            (None, Some(DecisionScoreArg::Lcb)) => DecisionScore::LowerConfidenceBound,
            (None, Some(DecisionScoreArg::Adjusted)) => DecisionScore::Adjusted,
            (None, Some(DecisionScoreArg::Raw)) => DecisionScore::Raw,
            (None, _) if args.use_lcb_score => DecisionScore::LowerConfidenceBound,
            (None, _) if args.use_adjusted_score => DecisionScore::Adjusted,
            _ => DecisionScore::Raw,
        },
        score_dispersion: match args.score_dispersion {
            DispersionArg::Std => ScoreDispersion::Std,
            DispersionArg::Mad => ScoreDispersion::Mad,
            DispersionArg::Iqr => ScoreDispersion::Iqr,
        },
        confidence_level: args.confidence_level,
    };
    config.validate().context("invalid configuration")?;
    let reports = match args.vote {
        VoteArg::Raw => score_all(observations.clone(), &config),
        VoteArg::Weighted => score_all_weighted(observations.clone(), &config),
    };

    if args.estimate_evaluator_reliability {
        let reliability = compute_evaluator_reliability(&observations, &reports);
        let mut sorted: Vec<_> = reliability.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (eval_id, r) in &sorted {
            eprintln!("reliability: {} = {:.4}", eval_id, r);
        }
    }

    let mut out = open_output(args.output.as_ref())?;
    match args.output_format {
        Format::Jsonl => {
            for report in &reports {
                let line = serde_json::to_string(report).context("serializing report")?;
                writeln!(out, "{line}")?;
            }
            if args.embed_stats {
                let mut meta = serde_json::Map::new();
                meta.insert("_quietset_stats".into(), serde_json::json!(true));
                if let Some(k) = compute_fleiss_kappa(&observations) {
                    meta.insert("fleiss_kappa".into(), serde_json::json!(k));
                }
                if let Some(a) = compute_krippendorff_alpha(&observations) {
                    meta.insert("krippendorff_alpha".into(), serde_json::json!(a));
                }
                writeln!(out, "{}", serde_json::to_string(&meta)?)?;
            }
        }
        Format::Csv => write_csv_reports(&reports, out)?,
    }
    Ok(())
}

fn run_filter(args: FilterArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let mut out = open_output(args.output.as_ref())?;
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.contains("\"_quietset_stats\"") {
            continue;
        }
        let report: StabilityReport = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                if args.skip_invalid {
                    eprintln!("warning: skipping line {}: {e}", i + 1);
                    continue;
                }
                return Err(e).with_context(|| format!("parsing JSONL at line {}", i + 1));
            }
        };
        if args
            .min_stability
            .is_some_and(|min| report.stability_score < min)
        {
            continue;
        }
        if args
            .max_disagreement
            .is_some_and(|max| report.disagreement_score > max)
        {
            continue;
        }
        if let Some(ref d) = args.decision {
            let want = match d {
                DecisionArg::Keep => Decision::Keep,
                DecisionArg::Review => Decision::Review,
                DecisionArg::Drop => Decision::Drop,
            };
            if report.decision != want {
                continue;
            }
        }
        if let Some(min) = args.min_label_lcb
            && report.label_agreement_lcb.is_none_or(|v| v < min)
        {
            continue;
        }
        if let Some(min) = args.min_confidence
            && report.confidence < min
        {
            continue;
        }
        if let Some(max) = args.max_score_mad
            && report.score_mad.is_some_and(|v| v > max)
        {
            continue;
        }
        if let Some(max) = args.max_score_iqr
            && report.score_iqr.is_some_and(|v| v > max)
        {
            continue;
        }
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn run_summary(args: SummaryArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let mut reports: Vec<StabilityReport> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.contains("\"_quietset_stats\"") {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(r) => reports.push(r),
            Err(e) => {
                if args.skip_invalid {
                    eprintln!("warning: skipping line {}: {e}", i + 1);
                } else {
                    return Err(e).with_context(|| format!("parsing JSONL at line {}", i + 1));
                }
            }
        }
    }
    if reports.is_empty() {
        anyhow::bail!("no records found");
    }

    let total = reports.len();
    let n_keep = reports
        .iter()
        .filter(|r| r.decision == Decision::Keep)
        .count();
    let n_review = reports
        .iter()
        .filter(|r| r.decision == Decision::Review)
        .count();
    let n_drop = reports
        .iter()
        .filter(|r| r.decision == Decision::Drop)
        .count();

    let mut scores: Vec<f64> = reports.iter().map(|r| r.stability_score).collect();
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let median = percentile(&scores, 0.50);
    let p10 = percentile(&scores, 0.10);
    let p90 = percentile(&scores, 0.90);

    let lcb_keep_demotions = reports
        .iter()
        .filter(|r| {
            r.stability_score >= args.keep_threshold
                && r.label_agreement_lcb
                    .map(|v| v < args.keep_threshold)
                    .unwrap_or(false)
        })
        .count();
    let has_lcb = reports.iter().any(|r| r.label_agreement_lcb.is_some());

    let mad_vals: Vec<f64> = reports.iter().filter_map(|r| r.score_mad).collect();
    let iqr_vals: Vec<f64> = reports.iter().filter_map(|r| r.score_iqr).collect();
    let score_mad_mean = if mad_vals.is_empty() {
        None
    } else {
        Some(mad_vals.iter().sum::<f64>() / mad_vals.len() as f64)
    };
    let score_iqr_mean = if iqr_vals.is_empty() {
        None
    } else {
        Some(iqr_vals.iter().sum::<f64>() / iqr_vals.len() as f64)
    };

    let mut driver_counts: HashMap<&'static str, usize> = HashMap::new();
    let unstable: Vec<&StabilityReport> = reports
        .iter()
        .filter(|r| r.decision != Decision::Keep)
        .collect();
    for r in &unstable {
        if let Some((name, _)) = r.components.weakest() {
            *driver_counts.entry(name).or_insert(0) += 1;
        }
    }
    let mut drivers: Vec<(&str, usize)> = driver_counts.into_iter().collect();
    drivers.sort_by_key(|d| Reverse(d.1));

    if args.json {
        let instability_map: serde_json::Map<String, serde_json::Value> = drivers
            .iter()
            .map(|(name, count)| (driver_label(name).to_string(), serde_json::json!(count)))
            .collect();
        let mut out = serde_json::json!({
            "total": total,
            "keep": n_keep, "review": n_review, "drop": n_drop,
            "keep_rate": n_keep as f64 / total as f64,
            "review_rate": n_review as f64 / total as f64,
            "drop_rate": n_drop as f64 / total as f64,
            "stability": { "mean": mean, "median": median, "p10": p10, "p90": p90 },
            "instability_drivers": instability_map,
        });
        if has_lcb {
            out["lcb_keep_demotions"] = serde_json::json!(lcb_keep_demotions);
        }
        if let Some(v) = score_mad_mean {
            out["score_mad_mean"] = serde_json::json!(v);
        }
        if let Some(v) = score_iqr_mean {
            out["score_iqr_mean"] = serde_json::json!(v);
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let pct = |n: usize| n as f64 / total as f64 * 100.0;
    println!("samples:        {:>8}", total);
    println!("  keep:         {:>8}  ({:.1}%)", n_keep, pct(n_keep));
    println!("  review:       {:>8}  ({:.1}%)", n_review, pct(n_review));
    println!("  drop:         {:>8}  ({:.1}%)", n_drop, pct(n_drop));
    if has_lcb {
        println!(
            "  lcb_keep_demotions:{:>8}  (stability_score >= {:.2}, label_agreement_lcb < {:.2})",
            lcb_keep_demotions, args.keep_threshold, args.keep_threshold
        );
    }
    println!();
    println!("stability_score:");
    println!("  mean:         {:>8.4}", mean);
    println!("  median:       {:>8.4}", median);
    println!("  p10 / p90:    {:.4} / {:.4}", p10, p90);
    if score_mad_mean.is_some() || score_iqr_mean.is_some() {
        println!();
        println!("score dispersion (mean across samples):");
        if let Some(v) = score_mad_mean {
            println!("  mad:          {:>8.4}", v);
        }
        if let Some(v) = score_iqr_mean {
            println!("  iqr:          {:>8.4}", v);
        }
    }
    if !drivers.is_empty() && !unstable.is_empty() {
        println!();
        println!("top instability drivers (review + drop samples):");
        for (name, count) in drivers.iter().take(6) {
            let pct_driver = *count as f64 / unstable.len() as f64 * 100.0;
            println!("  {:<24} {:.0}%", driver_label(name), pct_driver);
        }
    }
    Ok(())
}

fn run_explain(args: ExplainArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let mut found: Option<StabilityReport> = None;
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.contains("\"_quietset_stats\"") {
            continue;
        }
        let r: StabilityReport =
            serde_json::from_str(line).with_context(|| format!("parsing line {}", i + 1))?;
        if r.sample_id == args.sample_id {
            found = Some(r);
            break;
        }
    }
    let report =
        found.ok_or_else(|| anyhow::anyhow!("sample_id '{}' not found", args.sample_id))?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("sample_id:          {}", report.sample_id);
    println!("decision:           {}", report.decision);
    println!("n_observations:     {}", report.n_observations);
    println!("stability_score:    {:.4}", report.stability_score);
    println!("confidence:         {:.4}", report.confidence);
    println!("adjusted_score:     {:.4}", report.adjusted_stability_score);
    if let Some(v) = report.label_agreement_lcb {
        println!("label_agreement_lcb:{:.4}", v);
    }
    if let Some(m) = report.label_margin {
        println!("label_margin:       {:.4}", m);
    }
    if let Some(e) = report.label_entropy {
        println!("label_entropy:      {:.4}", e);
    }
    if report.score_mean.is_some() || report.score_mad.is_some() {
        println!();
        println!("score stats:");
        if let Some(v) = report.score_mean {
            println!("  mean:             {:.4}", v);
        }
        if let Some(v) = report.score_std {
            println!("  std:              {:.4}", v);
        }
        if let Some(v) = report.score_mad {
            println!("  mad:              {:.4}", v);
        }
        if let Some(v) = report.score_iqr {
            println!("  iqr:              {:.4}", v);
        }
    }
    println!();
    println!("components:");
    let c = &report.components;
    let weakest_name = c.weakest().map(|(n, _)| n);
    let print_comp = |name: &str, val: Option<f64>| {
        if let Some(v) = val {
            let bar: String = "█".repeat((v * 20.0) as usize);
            let marker = if weakest_name == Some(name) {
                "  ← weakest"
            } else {
                ""
            };
            println!("  {:<26} {:.4}  {}{}", name, v, bar, marker);
        }
    };
    print_comp("label", c.label);
    print_comp("score_consistency", c.score_consistency);
    print_comp("budget_robustness", c.budget_robustness);
    print_comp("seed_robustness", c.seed_robustness);
    print_comp("model_agreement", c.model_agreement);
    print_comp("evaluator_agreement", c.evaluator_agreement);
    Ok(())
}

fn run_compare(args: CompareArgs) -> Result<()> {
    let load = |path: &str| -> Result<HashMap<String, StabilityReport>> {
        let raw = read_input(path)?;
        let mut map = HashMap::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.contains("\"_quietset_stats\"") {
                continue;
            }
            let r: StabilityReport =
                serde_json::from_str(line).with_context(|| format!("parsing line {}", i + 1))?;
            map.insert(r.sample_id.clone(), r);
        }
        Ok(map)
    };

    let before = load(&args.before)?;
    let after = load(&args.after)?;

    let mut pairs: Vec<(&StabilityReport, &StabilityReport)> = Vec::new();
    for (id, b) in &before {
        if let Some(a) = after.get(id) {
            pairs.push((b, a));
        }
    }
    if pairs.is_empty() {
        anyhow::bail!("no matching sample_ids between the two files");
    }

    let decision_idx = |d: &Decision| match d {
        Decision::Keep => 0,
        Decision::Review => 1,
        Decision::Drop => 2,
    };
    let labels = ["keep", "review", "drop"];
    let mut matrix = [[0usize; 3]; 3];
    let mut mean_before = 0.0_f64;
    let mut mean_after = 0.0_f64;
    let mut regressions: Vec<(&str, f64, f64)> = Vec::new();

    for (b, a) in &pairs {
        matrix[decision_idx(&b.decision)][decision_idx(&a.decision)] += 1;
        mean_before += b.stability_score;
        mean_after += a.stability_score;
        if a.stability_score < b.stability_score {
            regressions.push((b.sample_id.as_str(), b.stability_score, a.stability_score));
        }
    }
    mean_before /= pairs.len() as f64;
    mean_after /= pairs.len() as f64;
    regressions.sort_by(|x, y| {
        (x.2 - x.1)
            .partial_cmp(&(y.2 - y.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    #[allow(clippy::type_complexity)]
    const COMP_FNS: &[(&str, fn(&quietset::StabilityComponents) -> Option<f64>)] = &[
        ("label", |c| c.label),
        ("score_consistency", |c| c.score_consistency),
        ("budget_robustness", |c| c.budget_robustness),
        ("seed_robustness", |c| c.seed_robustness),
        ("model_agreement", |c| c.model_agreement),
        ("evaluator_agreement", |c| c.evaluator_agreement),
    ];
    let mut comp_deltas: Vec<(&str, f64, f64)> = Vec::new();
    if args.components {
        for &(name, f) in COMP_FNS {
            let mut sb = 0.0f64;
            let mut cb = 0usize;
            let mut sa = 0.0f64;
            let mut ca = 0usize;
            for (b, a) in &pairs {
                if let Some(v) = f(&b.components) {
                    sb += v;
                    cb += 1;
                }
                if let Some(v) = f(&a.components) {
                    sa += v;
                    ca += 1;
                }
            }
            if cb > 0 && ca > 0 {
                comp_deltas.push((name, sb / cb as f64, sa / ca as f64));
            }
        }
    }

    if args.json {
        let mut transitions = serde_json::Map::new();
        for (i, from) in labels.iter().enumerate() {
            for (j, to) in labels.iter().enumerate() {
                if matrix[i][j] > 0 {
                    transitions.insert(format!("{from}_to_{to}"), serde_json::json!(matrix[i][j]));
                }
            }
        }
        let top_regressions: Vec<_> = regressions
            .iter()
            .take(args.top)
            .map(|(id, b, a)| {
                serde_json::json!({ "sample_id": id, "before": b, "after": a, "delta": a - b })
            })
            .collect();
        let mut out_json = serde_json::json!({
            "n_matched": pairs.len(),
            "mean_stability_before": mean_before,
            "mean_stability_after": mean_after,
            "transitions": transitions,
            "top_regressions": top_regressions,
        });
        if !comp_deltas.is_empty() {
            let deltas: serde_json::Map<String, serde_json::Value> = comp_deltas
                .iter()
                .map(|(n, b, a)| (n.to_string(), serde_json::json!(a - b)))
                .collect();
            out_json["component_deltas"] = serde_json::Value::Object(deltas);
        }
        println!("{}", serde_json::to_string_pretty(&out_json)?);
        return Ok(());
    }

    println!("matched samples:  {}", pairs.len());
    println!("mean stability:   {:.4} → {:.4}", mean_before, mean_after);
    println!();
    println!("decision transitions (before → after):");
    println!(
        "  {:>10}  {:>8}  {:>8}  {:>8}",
        "", "→keep", "→review", "→drop"
    );
    for (i, from) in labels.iter().enumerate() {
        println!(
            "  {:>10}  {:>8}  {:>8}  {:>8}",
            format!("{from}↓"),
            matrix[i][0],
            matrix[i][1],
            matrix[i][2]
        );
    }
    if !regressions.is_empty() {
        println!();
        println!("top {} regressions:", args.top.min(regressions.len()));
        for (id, b, a) in regressions.iter().take(args.top) {
            println!("  {}  {:.4} → {:.4}  (Δ{:.4})", id, b, a, a - b);
        }
    }
    if !comp_deltas.is_empty() {
        println!();
        println!("component deltas (mean before → after):");
        for (name, b, a) in &comp_deltas {
            let d = a - b;
            let marker = if d < -0.05 { "  ← regression" } else { "" };
            println!("  {:<26} {:.4} → {:.4}  ({:+.4}){}", name, b, a, d, marker);
        }
    }
    if let Some(ref pol) = args.policy_after {
        let kt = args.policy_keep_threshold;
        let dt = args.policy_drop_threshold;
        let policy_decide = |r: &StabilityReport| -> Decision {
            let score = match pol {
                DecisionScoreArg::Adjusted => r.adjusted_stability_score,
                DecisionScoreArg::Lcb => r.label_agreement_lcb.unwrap_or(0.0),
                DecisionScoreArg::Raw => r.stability_score,
            };
            if score >= kt {
                Decision::Keep
            } else if score <= dt {
                Decision::Drop
            } else {
                Decision::Review
            }
        };
        let pol_name = match pol {
            DecisionScoreArg::Raw => "raw",
            DecisionScoreArg::Adjusted => "adjusted",
            DecisionScoreArg::Lcb => "lcb",
        };
        let mut pol_matrix = [[0usize; 3]; 3];
        for (_, a) in &pairs {
            pol_matrix[decision_idx(&a.decision)][decision_idx(&policy_decide(a))] += 1;
        }
        let demoted = pol_matrix[0][1] + pol_matrix[0][2];
        let promoted = pol_matrix[1][0] + pol_matrix[2][0];
        println!();
        println!(
            "policy comparison: current → {} (keep_threshold={:.2}):",
            pol_name, kt
        );
        println!(
            "  {:>10}  {:>8}  {:>8}  {:>8}",
            "", "→keep", "→review", "→drop"
        );
        for (i, from) in labels.iter().enumerate() {
            println!(
                "  {:>10}  {:>8}  {:>8}  {:>8}",
                format!("{from}↓"),
                pol_matrix[i][0],
                pol_matrix[i][1],
                pol_matrix[i][2]
            );
        }
        println!("  demoted by policy: {}  promoted: {}", demoted, promoted);
    }
    Ok(())
}

fn run_audit(args: AuditArgs) -> Result<()> {
    // Load optional observation JSONL for agreement stats
    let agreement_obs: Option<Vec<Observation>> = if let Some(ref path) = args.observations {
        let raw = read_input(path)?;
        Some(parse_jsonl(&raw).context("parsing observations JSONL")?)
    } else {
        None
    };

    let raw = read_input(&args.input)?;
    let mut reports: Vec<StabilityReport> = Vec::new();
    let mut embedded_kappa: Option<f64> = None;
    let mut embedded_alpha: Option<f64> = None;
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.contains("\"_quietset_stats\"") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                embedded_kappa = v["fleiss_kappa"].as_f64().or(embedded_kappa);
                embedded_alpha = v["krippendorff_alpha"].as_f64().or(embedded_alpha);
            }
            continue;
        }
        match serde_json::from_str(line) {
            Ok(r) => reports.push(r),
            Err(e) => {
                if args.skip_invalid {
                    eprintln!("warning: skipping line {}: {e}", i + 1);
                } else {
                    return Err(e).with_context(|| format!("parsing JSONL at line {}", i + 1));
                }
            }
        }
    }
    if reports.is_empty() {
        anyhow::bail!("no records found");
    }

    let total = reports.len();
    let n_keep = reports
        .iter()
        .filter(|r| r.decision == Decision::Keep)
        .count();
    let n_review = reports
        .iter()
        .filter(|r| r.decision == Decision::Review)
        .count();
    let n_drop = reports
        .iter()
        .filter(|r| r.decision == Decision::Drop)
        .count();

    let mut scores: Vec<f64> = reports.iter().map(|r| r.stability_score).collect();
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let smean = scores.iter().sum::<f64>() / scores.len() as f64;
    let smedian = percentile(&scores, 0.50);
    let sp10 = percentile(&scores, 0.10);
    let sp90 = percentile(&scores, 0.90);

    let mad_vals: Vec<f64> = reports.iter().filter_map(|r| r.score_mad).collect();
    let iqr_vals: Vec<f64> = reports.iter().filter_map(|r| r.score_iqr).collect();
    let mad_mean = if mad_vals.is_empty() {
        None
    } else {
        Some(mad_vals.iter().sum::<f64>() / mad_vals.len() as f64)
    };
    let iqr_mean = if iqr_vals.is_empty() {
        None
    } else {
        Some(iqr_vals.iter().sum::<f64>() / iqr_vals.len() as f64)
    };

    let lcb_keep_demotions = reports
        .iter()
        .filter(|r| {
            r.stability_score >= args.keep_threshold
                && r.label_agreement_lcb
                    .is_some_and(|v| v < args.keep_threshold)
        })
        .count();
    let has_lcb = reports.iter().any(|r| r.label_agreement_lcb.is_some());

    let mut driver_counts: HashMap<&'static str, usize> = HashMap::new();
    let unstable: Vec<&StabilityReport> = reports
        .iter()
        .filter(|r| r.decision != Decision::Keep)
        .collect();
    for r in &unstable {
        if let Some((name, _)) = r.components.weakest() {
            *driver_counts.entry(name).or_insert(0) += 1;
        }
    }
    let mut drivers: Vec<(&str, usize)> = driver_counts.into_iter().collect();
    drivers.sort_by_key(|d| Reverse(d.1));

    let borderline_lo = (args.keep_threshold - 0.10).max(0.0);
    let borderline_hi = (args.keep_threshold + 0.10).min(1.0);
    let mut borderline: Vec<&StabilityReport> = reports
        .iter()
        .filter(|r| (borderline_lo..=borderline_hi).contains(&r.stability_score))
        .collect();
    borderline.sort_by(|a, b| {
        a.stability_score
            .partial_cmp(&b.stability_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut high_raw_low_lcb: Vec<&StabilityReport> = reports
        .iter()
        .filter(|r| {
            r.stability_score >= args.keep_threshold
                && r.label_agreement_lcb
                    .is_some_and(|v| v < args.keep_threshold)
        })
        .collect();
    high_raw_low_lcb.sort_by(|a, b| {
        a.label_agreement_lcb
            .unwrap_or(1.0)
            .partial_cmp(&b.label_agreement_lcb.unwrap_or(1.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut high_score_mad: Vec<&StabilityReport> =
        reports.iter().filter(|r| r.score_mad.is_some()).collect();
    high_score_mad.sort_by(|a, b| {
        b.score_mad
            .unwrap()
            .partial_cmp(&a.score_mad.unwrap())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut budget_sensitive: Vec<&StabilityReport> = reports
        .iter()
        .filter(|r| r.budget_sensitivity.is_some())
        .collect();
    budget_sensitive.sort_by(|a, b| {
        b.budget_sensitivity
            .unwrap()
            .partial_cmp(&a.budget_sensitivity.unwrap())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut seed_sensitive: Vec<&StabilityReport> = reports
        .iter()
        .filter(|r| r.seed_sensitivity.is_some())
        .collect();
    seed_sensitive.sort_by(|a, b| {
        b.seed_sensitivity
            .unwrap()
            .partial_cmp(&a.seed_sensitivity.unwrap())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let top = args.top;

    if args.json {
        let pct = |n: usize| n as f64 / total as f64;
        let mut out = serde_json::json!({
            "total": total,
            "keep": n_keep, "review": n_review, "drop": n_drop,
            "keep_rate": pct(n_keep), "review_rate": pct(n_review), "drop_rate": pct(n_drop),
            "stability": { "mean": smean, "median": smedian, "p10": sp10, "p90": sp90 },
            "instability_drivers": drivers.iter().map(|(n, c)| (driver_label(n).to_string(), serde_json::json!(c))).collect::<serde_json::Map<_,_>>(),
            "borderline": borderline.iter().take(top).map(|r| serde_json::json!({"sample_id": r.sample_id, "stability_score": r.stability_score, "decision": format!("{}", r.decision)})).collect::<Vec<_>>(),
            "high_raw_low_lcb": high_raw_low_lcb.iter().take(top).map(|r| serde_json::json!({"sample_id": r.sample_id, "stability_score": r.stability_score, "label_agreement_lcb": r.label_agreement_lcb})).collect::<Vec<_>>(),
            "high_score_mad": high_score_mad.iter().take(top).map(|r| serde_json::json!({"sample_id": r.sample_id, "score_mad": r.score_mad})).collect::<Vec<_>>(),
            "budget_sensitive": budget_sensitive.iter().take(top).map(|r| serde_json::json!({"sample_id": r.sample_id, "budget_sensitivity": r.budget_sensitivity})).collect::<Vec<_>>(),
            "seed_sensitive": seed_sensitive.iter().take(top).map(|r| serde_json::json!({"sample_id": r.sample_id, "seed_sensitivity": r.seed_sensitivity})).collect::<Vec<_>>(),
        });
        if has_lcb {
            out["lcb_keep_demotions"] = serde_json::json!(lcb_keep_demotions);
        }
        if let Some(v) = mad_mean {
            out["score_mad_mean"] = serde_json::json!(v);
        }
        if let Some(v) = iqr_mean {
            out["score_iqr_mean"] = serde_json::json!(v);
        }
        // --observations flag takes priority; fall back to embedded stats from --embed-stats
        let (kappa, alpha) = if let Some(ref obs) = agreement_obs {
            (compute_fleiss_kappa(obs), compute_krippendorff_alpha(obs))
        } else {
            (embedded_kappa, embedded_alpha)
        };
        if let Some(k) = kappa {
            out["fleiss_kappa"] = serde_json::json!(k);
        }
        if let Some(a) = alpha {
            out["krippendorff_alpha"] = serde_json::json!(a);
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let pct = |n: usize| n as f64 / total as f64 * 100.0;
    println!("=== quietset audit ===");
    println!("total:          {:>8}", total);
    println!("  keep:         {:>8}  ({:.1}%)", n_keep, pct(n_keep));
    println!("  review:       {:>8}  ({:.1}%)", n_review, pct(n_review));
    println!("  drop:         {:>8}  ({:.1}%)", n_drop, pct(n_drop));
    if has_lcb {
        println!(
            "  lcb_keep_demotions:{:>5}  (stability >= {:.2}, lcb < {:.2})",
            lcb_keep_demotions, args.keep_threshold, args.keep_threshold
        );
    }
    println!();
    println!("stability_score:");
    println!("  mean:         {:>8.4}", smean);
    println!("  median:       {:>8.4}", smedian);
    println!("  p10 / p90:    {:.4} / {:.4}", sp10, sp90);
    if mad_mean.is_some() || iqr_mean.is_some() {
        println!();
        println!("score dispersion (mean across samples):");
        if let Some(v) = mad_mean {
            println!("  mad:          {:>8.4}", v);
        }
        if let Some(v) = iqr_mean {
            println!("  iqr:          {:>8.4}", v);
        }
    }
    if !drivers.is_empty() && !unstable.is_empty() {
        println!();
        println!("top instability drivers:");
        for (name, count) in drivers.iter().take(6) {
            println!(
                "  {:<24} {:.0}%",
                driver_label(name),
                *count as f64 / unstable.len() as f64 * 100.0
            );
        }
    }
    if !borderline.is_empty() {
        println!();
        println!(
            "--- borderline ({:.2} <= stability <= {:.2}, top {}) ---",
            borderline_lo,
            borderline_hi,
            borderline.len().min(top)
        );
        for r in borderline.iter().take(top) {
            println!(
                "  {:<36} {:.4}  {}",
                r.sample_id, r.stability_score, r.decision
            );
        }
    }
    if !high_raw_low_lcb.is_empty() {
        println!();
        println!(
            "--- high_raw_low_lcb (stability >= {:.2}, lcb < {:.2}, top {}) ---",
            args.keep_threshold,
            args.keep_threshold,
            high_raw_low_lcb.len().min(top)
        );
        for r in high_raw_low_lcb.iter().take(top) {
            println!(
                "  {:<36} stability={:.4}  lcb={:.4}",
                r.sample_id,
                r.stability_score,
                r.label_agreement_lcb.unwrap_or(0.0)
            );
        }
    }
    if !high_score_mad.is_empty() {
        println!();
        println!(
            "--- high_score_mad (top {}) ---",
            high_score_mad.len().min(top)
        );
        for r in high_score_mad.iter().take(top) {
            println!(
                "  {:<36} mad={:.4}",
                r.sample_id,
                r.score_mad.unwrap_or(0.0)
            );
        }
    }
    if !budget_sensitive.is_empty() {
        println!();
        println!(
            "--- budget_sensitive (top {}) ---",
            budget_sensitive.len().min(top)
        );
        for r in budget_sensitive.iter().take(top) {
            println!(
                "  {:<36} budget_sensitivity={:.4}",
                r.sample_id,
                r.budget_sensitivity.unwrap_or(0.0)
            );
        }
    }
    if !seed_sensitive.is_empty() {
        println!();
        println!(
            "--- seed_sensitive (top {}) ---",
            seed_sensitive.len().min(top)
        );
        for r in seed_sensitive.iter().take(top) {
            println!(
                "  {:<36} seed_sensitivity={:.4}",
                r.sample_id,
                r.seed_sensitivity.unwrap_or(0.0)
            );
        }
    }
    let (text_kappa, text_alpha) = if let Some(ref obs) = agreement_obs {
        (compute_fleiss_kappa(obs), compute_krippendorff_alpha(obs))
    } else {
        (embedded_kappa, embedded_alpha)
    };
    if text_kappa.is_some() || text_alpha.is_some() {
        println!();
        println!("dataset agreement:");
        if let Some(k) = text_kappa {
            println!("  fleiss_kappa:         {:.4}", k);
        }
        if let Some(a) = text_alpha {
            println!("  krippendorff_alpha:   {:.4}", a);
        }
    }
    Ok(())
}

fn run_select(args: SelectArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let mut lines_and_reports: Vec<(String, StabilityReport)> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.contains("\"_quietset_stats\"") {
            continue;
        }
        match serde_json::from_str::<StabilityReport>(line) {
            Ok(r) => lines_and_reports.push((line.to_string(), r)),
            Err(e) => {
                if args.skip_invalid {
                    eprintln!("warning: skipping line {}: {e}", i + 1);
                } else {
                    return Err(e).with_context(|| format!("parsing JSONL at line {}", i + 1));
                }
            }
        }
    }
    if lines_and_reports.is_empty() {
        anyhow::bail!("no records found");
    }

    let kt = args.keep_threshold;
    // Sort indices by the relevant field for the chosen class
    let mut indices: Vec<usize> = (0..lines_and_reports.len()).collect();

    match args.class {
        SelectClass::Borderline => {
            let lo = (kt - 0.10).max(0.0);
            let hi = (kt + 0.10).min(1.0);
            indices.retain(|&i| {
                let s = lines_and_reports[i].1.stability_score;
                (lo..=hi).contains(&s)
            });
            indices.sort_by(|&a, &b| {
                lines_and_reports[a]
                    .1
                    .stability_score
                    .partial_cmp(&lines_and_reports[b].1.stability_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SelectClass::HighDisagreement => {
            indices.sort_by(|&a, &b| {
                lines_and_reports[b]
                    .1
                    .disagreement_score
                    .partial_cmp(&lines_and_reports[a].1.disagreement_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SelectClass::BudgetSensitive => {
            indices.retain(|&i| lines_and_reports[i].1.budget_sensitivity.is_some());
            indices.sort_by(|&a, &b| {
                lines_and_reports[b]
                    .1
                    .budget_sensitivity
                    .unwrap()
                    .partial_cmp(&lines_and_reports[a].1.budget_sensitivity.unwrap())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SelectClass::SeedSensitive => {
            indices.retain(|&i| lines_and_reports[i].1.seed_sensitivity.is_some());
            indices.sort_by(|&a, &b| {
                lines_and_reports[b]
                    .1
                    .seed_sensitivity
                    .unwrap()
                    .partial_cmp(&lines_and_reports[a].1.seed_sensitivity.unwrap())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SelectClass::HighRawLowLcb => {
            indices.retain(|&i| {
                let r = &lines_and_reports[i].1;
                r.stability_score >= kt && r.label_agreement_lcb.is_some_and(|v| v < kt)
            });
            indices.sort_by(|&a, &b| {
                lines_and_reports[a]
                    .1
                    .label_agreement_lcb
                    .unwrap_or(1.0)
                    .partial_cmp(&lines_and_reports[b].1.label_agreement_lcb.unwrap_or(1.0))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        SelectClass::HighScoreMad => {
            indices.retain(|&i| lines_and_reports[i].1.score_mad.is_some());
            indices.sort_by(|&a, &b| {
                lines_and_reports[b]
                    .1
                    .score_mad
                    .unwrap()
                    .partial_cmp(&lines_and_reports[a].1.score_mad.unwrap())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    let take = args.top.unwrap_or(indices.len());
    for &i in indices.iter().take(take) {
        println!("{}", lines_and_reports[i].0);
    }
    Ok(())
}

fn run_recommend(args: RecommendArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let mut reports: Vec<StabilityReport> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.contains("\"_quietset_stats\"") {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(r) => reports.push(r),
            Err(e) => {
                if args.skip_invalid {
                    eprintln!("warning: skipping line {}: {e}", i + 1);
                } else {
                    return Err(e).with_context(|| format!("parsing JSONL at line {}", i + 1));
                }
            }
        }
    }
    if reports.is_empty() {
        anyhow::bail!("no records found");
    }

    for r in &reports {
        if args.unstable_only && r.decision == Decision::Keep {
            // still emit if LCB risk is present
            let lcb_risk = r
                .label_agreement_lcb
                .is_some_and(|v| v < args.keep_threshold);
            if !lcb_risk {
                continue;
            }
        }

        // Priority-ordered rules — emit the first matching one
        let rec: Option<serde_json::Value> = if r.stability_score >= args.keep_threshold
            && r.label_agreement_lcb
                .is_some_and(|v| v < args.keep_threshold)
        {
            Some(serde_json::json!({
                "sample_id": r.sample_id,
                "reason": "high_raw_low_lcb",
                "recommended_action": "add_observations",
                "stability_score": r.stability_score,
                "label_agreement_lcb": r.label_agreement_lcb,
                "n_observations": r.n_observations,
            }))
        } else if r.evaluator_agreement.is_some_and(|v| v < 0.7) {
            Some(serde_json::json!({
                "sample_id": r.sample_id,
                "reason": "low_evaluator_agreement",
                "recommended_action": "add_evaluators",
                "evaluator_agreement": r.evaluator_agreement,
            }))
        } else if r.seed_sensitivity.is_some_and(|v| v > 0.3) {
            Some(serde_json::json!({
                "sample_id": r.sample_id,
                "reason": "high_seed_sensitivity",
                "recommended_action": "add_seeds",
                "seed_sensitivity": r.seed_sensitivity,
            }))
        } else if r.budget_sensitivity.is_some_and(|v| v > 0.3) {
            Some(serde_json::json!({
                "sample_id": r.sample_id,
                "reason": "high_budget_sensitivity",
                "recommended_action": "increase_budget",
                "budget_sensitivity": r.budget_sensitivity,
            }))
        } else if r.model_agreement.is_some_and(|v| v < 0.7) {
            Some(serde_json::json!({
                "sample_id": r.sample_id,
                "reason": "low_model_agreement",
                "recommended_action": "add_models",
                "model_agreement": r.model_agreement,
            }))
        } else {
            None
        };

        if let Some(rec) = rec {
            if args.text {
                let reason = rec["reason"].as_str().unwrap_or("");
                let action = rec["recommended_action"].as_str().unwrap_or("");
                let detail = match reason {
                    "high_raw_low_lcb" => format!(
                        "lcb={:.3}  n={}",
                        rec["label_agreement_lcb"].as_f64().unwrap_or(0.0),
                        rec["n_observations"].as_u64().unwrap_or(0)
                    ),
                    "low_evaluator_agreement" => format!(
                        "evaluator_agreement={:.3}",
                        rec["evaluator_agreement"].as_f64().unwrap_or(0.0)
                    ),
                    "high_seed_sensitivity" => format!(
                        "seed_sensitivity={:.3}",
                        rec["seed_sensitivity"].as_f64().unwrap_or(0.0)
                    ),
                    "high_budget_sensitivity" => format!(
                        "budget_sensitivity={:.3}",
                        rec["budget_sensitivity"].as_f64().unwrap_or(0.0)
                    ),
                    "low_model_agreement" => format!(
                        "model_agreement={:.3}",
                        rec["model_agreement"].as_f64().unwrap_or(0.0)
                    ),
                    _ => String::new(),
                };
                println!(
                    "{:<36} {:>20}  →  {}  ({})",
                    rec["sample_id"].as_str().unwrap_or(""),
                    reason,
                    action,
                    detail
                );
            } else {
                println!("{}", serde_json::to_string(&rec)?);
            }
        }
    }
    Ok(())
}

fn run_stable_wrong_risk(args: StableWrongRiskArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let observations = if args.skip_invalid {
        let mut obs = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.contains("\"_quietset_stats\"") {
                continue;
            }
            match serde_json::from_str::<Observation>(line) {
                Ok(o) => match o.validate(i + 1) {
                    Ok(()) => obs.push(o),
                    Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
                },
                Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
            }
        }
        obs
    } else {
        parse_jsonl(&raw).context("parsing JSONL")?
    };
    if observations.is_empty() {
        anyhow::bail!("no observations found");
    }

    let gold_map: std::collections::HashMap<String, String> = observations
        .iter()
        .filter_map(|o| {
            o.gold_label
                .as_deref()
                .map(|g| (o.sample_id.clone(), g.to_string()))
        })
        .collect();
    if gold_map.is_empty() {
        anyhow::bail!("no gold_label found; stable_wrong_risk requires gold_label on observations");
    }

    let config = ScoreConfig {
        thresholds: Thresholds {
            keep: args.keep_threshold,
            drop: 0.40,
        },
        ..ScoreConfig::default()
    };
    let reports = score_all(observations, &config);

    let n_total = reports.len();
    let n_keep = reports
        .iter()
        .filter(|r| r.decision == Decision::Keep)
        .count();

    let mut stable_wrong: Vec<serde_json::Value> = reports
        .iter()
        .filter(|r| r.decision == Decision::Keep)
        .filter(|r| {
            r.majority_label
                .as_deref()
                .and_then(|m| gold_map.get(&r.sample_id).map(|g| m != g.as_str()))
                .unwrap_or(false)
        })
        .map(|r| {
            serde_json::json!({
                "sample_id": r.sample_id,
                "stability_score": r.stability_score,
                "majority_label": r.majority_label,
                "gold_label": gold_map.get(&r.sample_id),
            })
        })
        .collect();

    stable_wrong.sort_by(|a, b| {
        b["stability_score"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["stability_score"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let n_stable_wrong = stable_wrong.len();
    let rate = if n_keep > 0 {
        n_stable_wrong as f64 / n_keep as f64
    } else {
        0.0
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "n_total": n_total,
            "n_keep": n_keep,
            "n_stable_wrong": n_stable_wrong,
            "stable_wrong_rate_among_keep": rate,
            "samples": stable_wrong.iter().take(args.top).collect::<Vec<_>>(),
        }))?
    );
    Ok(())
}

fn run_calibrate(args: CalibrateArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let observations = if args.skip_invalid {
        let mut obs = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.contains("\"_quietset_stats\"") {
                continue;
            }
            match serde_json::from_str::<Observation>(line) {
                Ok(o) => match o.validate(i + 1) {
                    Ok(()) => obs.push(o),
                    Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
                },
                Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
            }
        }
        obs
    } else {
        parse_jsonl(&raw).context("parsing JSONL")?
    };
    if observations.is_empty() {
        anyhow::bail!("no observations found");
    }

    let decision_score = match args.decision_score {
        Some(DecisionScoreArg::Lcb) => DecisionScore::LowerConfidenceBound,
        Some(DecisionScoreArg::Adjusted) => DecisionScore::Adjusted,
        Some(DecisionScoreArg::Raw) | None => DecisionScore::Raw,
    };

    let result = compute_calibration(
        &observations,
        &decision_score,
        args.confidence_level,
        args.target_precision,
        args.target_coverage,
        args.drop_threshold,
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "calibration failed: either no gold_label is present in observations, or no \
         threshold in [0.50, 0.99] meets the target. Try --target-precision with a lower value."
        )
    })?;

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "decision_score":      result.decision_score_name,
            "keep_threshold":      result.keep_threshold,
            "drop_threshold":      result.drop_threshold,
            "achieved_precision":  result.achieved_precision,
            "coverage":            result.coverage,
            "n_keep":              result.n_keep,
            "n_total":             result.n_total,
        }))?
    );
    Ok(())
}

fn run_reliability(args: ReliabilityArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let observations = if args.skip_invalid {
        let mut obs = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.contains("\"_quietset_stats\"") {
                continue;
            }
            match serde_json::from_str::<Observation>(line) {
                Ok(o) => match o.validate(i + 1) {
                    Ok(()) => obs.push(o),
                    Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
                },
                Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
            }
        }
        obs
    } else {
        parse_jsonl(&raw).context("parsing JSONL")?
    };
    if observations.is_empty() {
        anyhow::bail!("no observations found");
    }
    let reports = score_all(observations.clone(), &ScoreConfig::default());
    let reliability = compute_evaluator_reliability(&observations, &reports);
    let mut sorted: Vec<_> = reliability.into_iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let gold_map: std::collections::HashMap<&str, &str> = observations
        .iter()
        .filter_map(|o| o.gold_label.as_deref().map(|g| (o.sample_id.as_str(), g)))
        .collect();
    let has_gold = !gold_map.is_empty();

    // confusion[eval_id][predicted][gold] = count
    let mut confusion_data: std::collections::HashMap<
        String,
        std::collections::HashMap<String, std::collections::HashMap<String, usize>>,
    > = std::collections::HashMap::new();
    if has_gold {
        for obs in &observations {
            if let (Some(eval_id), Some(label)) =
                (obs.evaluator_id.as_deref(), obs.label.as_deref())
                && let Some(&gold) = gold_map.get(obs.sample_id.as_str())
            {
                *confusion_data
                    .entry(eval_id.to_string())
                    .or_default()
                    .entry(label.to_string())
                    .or_default()
                    .entry(gold.to_string())
                    .or_insert(0) += 1;
            }
        }
    }

    for (eval_id, r) in &sorted {
        let line = if has_gold {
            let conf = confusion_data
                .get(eval_id.as_str())
                .cloned()
                .unwrap_or_default();
            serde_json::json!({ "evaluator_id": eval_id, "reliability": r, "confusion": conf })
        } else {
            serde_json::json!({ "evaluator_id": eval_id, "reliability": r })
        };
        println!("{}", serde_json::to_string(&line)?);
    }
    let kappa = compute_fleiss_kappa(&observations);
    let alpha = compute_krippendorff_alpha(&observations);
    if kappa.is_some() || alpha.is_some() {
        let mut summary = serde_json::Map::new();
        if let Some(k) = kappa {
            summary.insert("fleiss_kappa".into(), serde_json::json!(k));
        }
        if let Some(a) = alpha {
            summary.insert("krippendorff_alpha".into(), serde_json::json!(a));
        }
        println!("{}", serde_json::to_string(&summary)?);
    }
    Ok(())
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn driver_label(name: &str) -> &str {
    match name {
        "label" => "label disagreement",
        "score_consistency" => "score variance",
        "budget_robustness" => "budget sensitivity",
        "seed_robustness" => "seed sensitivity",
        "model_agreement" => "model disagreement",
        "evaluator_agreement" => "evaluator disagreement",
        other => other,
    }
}

fn run_policy(args: PolicyArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let observations = if args.skip_invalid {
        let mut obs = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.contains("\"_quietset_stats\"") {
                continue;
            }
            match serde_json::from_str::<Observation>(line) {
                Ok(o) => match o.validate(i + 1) {
                    Ok(()) => obs.push(o),
                    Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
                },
                Err(e) => eprintln!("warning: skipping line {}: {e}", i + 1),
            }
        }
        obs
    } else {
        parse_jsonl(&raw).context("parsing JSONL")?
    };
    if observations.is_empty() {
        anyhow::bail!("no observations found");
    }

    let decision_score = match args.decision_score {
        Some(DecisionScoreArg::Lcb) => DecisionScore::LowerConfidenceBound,
        Some(DecisionScoreArg::Adjusted) => DecisionScore::Adjusted,
        Some(DecisionScoreArg::Raw) | None => DecisionScore::Raw,
    };

    // Score with keep=0 to obtain all samples' stability scores
    let config = ScoreConfig {
        thresholds: Thresholds {
            keep: 0.0,
            drop: -1.0,
        },
        decision_score: decision_score.clone(),
        confidence_level: args.confidence_level,
        ..ScoreConfig::default()
    };
    let reports = score_all(observations.clone(), &config);
    let n_total = reports.len();
    if n_total == 0 {
        anyhow::bail!("no samples scored");
    }

    let gold: std::collections::HashMap<&str, &str> = observations
        .iter()
        .filter_map(|o| o.gold_label.as_deref().map(|g| (o.sample_id.as_str(), g)))
        .collect();
    let has_gold = !gold.is_empty();

    let score_val = |r: &StabilityReport| match &decision_score {
        DecisionScore::Adjusted => r.adjusted_stability_score,
        _ => r.stability_score,
    };

    struct Row {
        threshold: f64,
        n_keep: usize,
        coverage: f64,
        precision: Option<f64>,
        stable_wrong_rate: Option<f64>,
    }

    let mut rows: Vec<Row> = Vec::new();
    for i in 0..=49usize {
        let t = 0.99 - i as f64 * 0.01;
        let kept: Vec<&StabilityReport> = reports.iter().filter(|r| score_val(r) >= t).collect();
        let n_keep = kept.len();
        let coverage = n_keep as f64 / n_total as f64;

        let (precision, stable_wrong_rate) = if has_gold && n_keep > 0 {
            let matches = kept
                .iter()
                .filter(|r| {
                    gold.get(r.sample_id.as_str())
                        .and_then(|&g| r.majority_label.as_deref().map(|m| m == g))
                        .unwrap_or(false)
                })
                .count();
            let wrong = kept
                .iter()
                .filter(|r| {
                    r.majority_label
                        .as_deref()
                        .and_then(|ml| gold.get(r.sample_id.as_str()).map(|&g| ml != g))
                        .unwrap_or(false)
                })
                .count();
            (
                Some(matches as f64 / n_keep as f64),
                Some(wrong as f64 / n_keep as f64),
            )
        } else {
            (None, None)
        };

        rows.push(Row {
            threshold: t,
            n_keep,
            coverage,
            precision,
            stable_wrong_rate,
        });
    }

    // Find best thresholds (loosest that meets target, from high→low)
    let best_prec_idx = args.target_precision.and_then(|tp| {
        rows.iter()
            .rposition(|r| r.precision.is_some_and(|p| p >= tp))
    });
    let best_cov_idx = args
        .target_coverage
        .and_then(|tc| rows.iter().rposition(|r| r.coverage >= tc));
    let best_idx = best_prec_idx.or(best_cov_idx);

    if args.json {
        for (i, row) in rows.iter().enumerate() {
            let mut obj = serde_json::Map::new();
            obj.insert("threshold".into(), serde_json::json!(row.threshold));
            obj.insert("n_keep".into(), serde_json::json!(row.n_keep));
            obj.insert("coverage".into(), serde_json::json!(row.coverage));
            if let Some(p) = row.precision {
                obj.insert("precision".into(), serde_json::json!(p));
            }
            if let Some(s) = row.stable_wrong_rate {
                obj.insert("stable_wrong_rate".into(), serde_json::json!(s));
            }
            if Some(i) == best_idx {
                obj.insert("best".into(), serde_json::json!(true));
            }
            println!("{}", serde_json::to_string(&obj)?);
        }
    } else {
        if has_gold {
            println!(
                "{:<9}  {:>6}  {:>8}  {:>9}  {:>18}",
                "threshold", "n_keep", "coverage", "precision", "stable_wrong_rate"
            );
        } else {
            println!("{:<9}  {:>6}  {:>8}", "threshold", "n_keep", "coverage");
        }
        for (i, row) in rows.iter().enumerate() {
            let marker = if Some(i) == best_idx { " ←" } else { "" };
            if has_gold {
                println!(
                    "{:<9.2}  {:>6}  {:>8.3}  {:>9}  {:>18}{}",
                    row.threshold,
                    row.n_keep,
                    row.coverage,
                    row.precision
                        .map(|p| format!("{p:.3}"))
                        .unwrap_or_else(|| "-".into()),
                    row.stable_wrong_rate
                        .map(|r| format!("{r:.3}"))
                        .unwrap_or_else(|| "-".into()),
                    marker
                );
            } else {
                println!(
                    "{:<9.2}  {:>6}  {:>8.3}{}",
                    row.threshold, row.n_keep, row.coverage, marker
                );
            }
        }
        if let (Some(tp), Some(idx)) = (args.target_precision, best_prec_idx) {
            println!(
                "\nbest (precision >= {tp:.2}): threshold={:.2}, coverage={:.3}",
                rows[idx].threshold, rows[idx].coverage
            );
        } else if args.target_precision.is_some() {
            println!("\nno threshold in [0.50, 0.99] meets precision target");
        }
        if let (Some(tc), Some(idx)) = (args.target_coverage, best_cov_idx) {
            println!(
                "\nbest (coverage >= {tc:.2}): threshold={:.2}, n_keep={}",
                rows[idx].threshold, rows[idx].n_keep
            );
        } else if args.target_coverage.is_some() {
            println!("\nno threshold in [0.50, 0.99] meets coverage target");
        }
    }
    Ok(())
}

fn run_active_review(args: ActiveReviewArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let mut reports: Vec<StabilityReport> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.contains("\"_quietset_stats\"") {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(r) => reports.push(r),
            Err(e) => {
                if args.skip_invalid {
                    eprintln!("warning: skipping line {}: {e}", i + 1);
                } else {
                    return Err(e).with_context(|| format!("parsing JSONL at line {}", i + 1));
                }
            }
        }
    }
    if reports.is_empty() {
        anyhow::bail!("no records found");
    }

    struct Entry {
        urgency: f64,
        sample_id: String,
        value: serde_json::Value,
    }

    let mut entries: Vec<Entry> = Vec::new();
    for r in &reports {
        if args.unstable_only && r.decision == Decision::Keep {
            continue;
        }

        let signals: &[(&'static str, f64, &'static str)] = &[
            (
                "low_lcb",
                r.label_agreement_lcb
                    .map(|v| (1.0 - v) * args.weight_lcb)
                    .unwrap_or(0.0),
                "add_observations",
            ),
            (
                "high_entropy",
                r.label_entropy
                    .map(|v| v * args.weight_entropy)
                    .unwrap_or(0.0),
                "diversify_evaluators",
            ),
            (
                "high_score_mad",
                r.score_mad
                    .map(|v| v.min(1.0) * args.weight_score_mad)
                    .unwrap_or(0.0),
                "reduce_score_variance",
            ),
            (
                "high_budget_sensitivity",
                r.budget_sensitivity
                    .map(|v| v * args.weight_budget_sensitivity)
                    .unwrap_or(0.0),
                "add_budget",
            ),
            (
                "high_seed_sensitivity",
                r.seed_sensitivity
                    .map(|v| v * args.weight_seed_sensitivity)
                    .unwrap_or(0.0),
                "add_seeds",
            ),
        ];

        let total_w: f64 = [
            r.label_agreement_lcb
                .map(|_| args.weight_lcb)
                .unwrap_or(0.0),
            r.label_entropy.map(|_| args.weight_entropy).unwrap_or(0.0),
            r.score_mad.map(|_| args.weight_score_mad).unwrap_or(0.0),
            r.budget_sensitivity
                .map(|_| args.weight_budget_sensitivity)
                .unwrap_or(0.0),
            r.seed_sensitivity
                .map(|_| args.weight_seed_sensitivity)
                .unwrap_or(0.0),
        ]
        .iter()
        .sum();

        let raw_sum: f64 = signals.iter().map(|(_, v, _)| v).sum();
        let urgency = if total_w > 0.0 {
            raw_sum / total_w
        } else {
            0.0
        };

        let (primary_reason, _, suggested_action) = signals
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        let mut obj = serde_json::Map::new();
        obj.insert("sample_id".into(), serde_json::json!(r.sample_id));
        obj.insert("urgency_score".into(), serde_json::json!(urgency));
        obj.insert("primary_reason".into(), serde_json::json!(primary_reason));
        obj.insert(
            "suggested_action".into(),
            serde_json::json!(suggested_action),
        );
        if let Some(v) = r.label_agreement_lcb {
            obj.insert("label_agreement_lcb".into(), serde_json::json!(v));
        }
        if let Some(v) = r.label_entropy {
            obj.insert("label_entropy".into(), serde_json::json!(v));
        }
        if let Some(v) = r.budget_sensitivity {
            obj.insert("budget_sensitivity".into(), serde_json::json!(v));
        }
        if let Some(v) = r.seed_sensitivity {
            obj.insert("seed_sensitivity".into(), serde_json::json!(v));
        }

        entries.push(Entry {
            urgency,
            sample_id: r.sample_id.clone(),
            value: serde_json::Value::Object(obj),
        });
    }

    entries.sort_by(|a, b| {
        b.urgency
            .partial_cmp(&a.urgency)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.sample_id.cmp(&b.sample_id))
    });

    let take = args.top.unwrap_or(entries.len());
    for e in entries.iter().take(take) {
        println!("{}", serde_json::to_string(&e.value)?);
    }
    Ok(())
}
