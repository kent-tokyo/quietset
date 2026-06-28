use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use std::cmp::Reverse;
use std::collections::HashMap;

use quietset::{
    Decision, DecisionScore, MinRequirements, Observation, ScoreConfig, ScoreWeights,
    StabilityReport, Thresholds, compute_evaluator_reliability, parse_csv, parse_jsonl, score_all,
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

    /// Use adjusted_stability_score (confidence-adjusted) for keep/review/drop decisions.
    #[arg(long)]
    use_adjusted_score: bool,

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

    /// Weight for label_agreement in stability_score (0 = exclude).
    #[arg(long, default_value_t = 1.0)]
    weight_labels: f64,

    /// Weight for score stability (1 - normalized_score_std) in stability_score.
    #[arg(long, default_value_t = 1.0)]
    weight_scores: f64,

    /// Weight for budget stability (1 - budget_sensitivity) in stability_score.
    #[arg(long, default_value_t = 1.0)]
    weight_budget: f64,

    /// Weight for seed stability (1 - seed_sensitivity) in stability_score.
    #[arg(long, default_value_t = 1.0)]
    weight_seed: f64,

    /// Weight for model_agreement in stability_score.
    #[arg(long, default_value_t = 1.0)]
    weight_models: f64,

    /// Weight for evaluator_agreement in stability_score.
    #[arg(long, default_value_t = 1.0)]
    weight_evaluators: f64,
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
        "label_margin",
        "label_entropy",
        "score_mean",
        "score_std",
        "score_range",
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
            &r.label_margin
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.label_entropy
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            &r.score_mean.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_std.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_range.map(|v| format!("{v:.6}")).unwrap_or_default(),
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
                    if line.is_empty() {
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
            Format::Csv => parse_csv(raw.as_bytes()).context("parsing CSV")?,
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
    let config = ScoreConfig {
        score_scale: args.score_scale,
        thresholds: Thresholds {
            keep: args.keep_threshold,
            drop: args.drop_threshold,
        },
        weights: ScoreWeights {
            label_agreement: args.weight_labels,
            score_stability: args.weight_scores,
            budget_stability: args.weight_budget,
            seed_stability: args.weight_seed,
            model_agreement: args.weight_models,
            evaluator_agreement: args.weight_evaluators,
        },
        confidence_k: args.confidence_k,
        min_requirements: MinRequirements {
            observations: args.min_observations_keep,
            evaluators: args.min_evaluators_keep,
            seeds: args.min_seeds_keep,
            budgets: args.min_budgets_keep,
            models: args.min_models_keep,
        },
        decision_score: if args.use_adjusted_score {
            DecisionScore::Adjusted
        } else {
            DecisionScore::Raw
        },
    };
    config.validate().context("invalid configuration")?;
    let reports = score_all(observations.clone(), &config);

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
        if line.is_empty() {
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
        writeln!(out, "{line}")?;
    }
    Ok(())
}

fn run_summary(args: SummaryArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let mut reports: Vec<StabilityReport> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
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
        let out = serde_json::json!({
            "total": total,
            "keep": n_keep, "review": n_review, "drop": n_drop,
            "keep_rate": n_keep as f64 / total as f64,
            "review_rate": n_review as f64 / total as f64,
            "drop_rate": n_drop as f64 / total as f64,
            "stability": { "mean": mean, "median": median, "p10": p10, "p90": p90 },
            "instability_drivers": instability_map,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let pct = |n: usize| n as f64 / total as f64 * 100.0;
    println!("samples:        {:>8}", total);
    println!("  keep:         {:>8}  ({:.1}%)", n_keep, pct(n_keep));
    println!("  review:       {:>8}  ({:.1}%)", n_review, pct(n_review));
    println!("  drop:         {:>8}  ({:.1}%)", n_drop, pct(n_drop));
    println!();
    println!("stability_score:");
    println!("  mean:         {:>8.4}", mean);
    println!("  median:       {:>8.4}", median);
    println!("  p10 / p90:    {:.4} / {:.4}", p10, p90);
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
    let report: StabilityReport = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .find_map(|line| {
            serde_json::from_str::<StabilityReport>(line)
                .ok()
                .filter(|r| r.sample_id == args.sample_id)
        })
        .ok_or_else(|| anyhow::anyhow!("sample_id '{}' not found", args.sample_id))?;

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
    if let Some(m) = report.label_margin {
        println!("label_margin:       {:.4}", m);
    }
    if let Some(e) = report.label_entropy {
        println!("label_entropy:      {:.4}", e);
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
            if line.is_empty() {
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
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "n_matched": pairs.len(),
                "mean_stability_before": mean_before,
                "mean_stability_after": mean_after,
                "transitions": transitions,
                "top_regressions": top_regressions,
            }))?
        );
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
    Ok(())
}

fn run_reliability(args: ReliabilityArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let observations = if args.skip_invalid {
        let mut obs = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
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
    for (eval_id, r) in &sorted {
        let line = serde_json::json!({ "evaluator_id": eval_id, "reliability": r });
        println!("{}", serde_json::to_string(&line)?);
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
