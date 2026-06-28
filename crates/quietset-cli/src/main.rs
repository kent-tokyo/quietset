use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use std::cmp::Reverse;
use std::collections::HashMap;

use quietset::{
    Decision, Observation, ScoreConfig, ScoreWeights, StabilityReport, Thresholds, parse_csv,
    parse_jsonl, score_all,
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
        "score_mean",
        "score_std",
        "score_range",
        "budget_sensitivity",
        "seed_sensitivity",
        "model_agreement",
        "evaluator_agreement",
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
            &r.score_mean.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_std.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.score_range.map(|v| format!("{v:.6}")).unwrap_or_default(),
            &r.budget_sensitivity
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
    };
    config.validate().context("invalid configuration")?;
    let reports = score_all(observations, &config);
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

    // Instability drivers: count the weakest component for each review+drop sample
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
