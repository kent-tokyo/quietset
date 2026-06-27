use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use quietset::{parse_csv, parse_jsonl, score_all, Decision, ScoreConfig, Thresholds};

#[derive(Parser)]
#[command(name = "quietset", about = "Filter datasets by label stability")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Score observations and output StabilityReports as JSONL
    Score(ScoreArgs),
    /// Filter scored JSONL by stability/decision thresholds
    Filter(FilterArgs),
}

#[derive(clap::Args)]
struct ScoreArgs {
    /// Input file (JSONL or CSV). Use '-' for stdin.
    #[arg(default_value = "-")]
    input: String,

    #[arg(long, default_value = "jsonl", value_enum)]
    format: Format,

    /// Output file. Default: stdout.
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// Scale for normalizing score_std and sensitivity metrics.
    #[arg(long, default_value_t = 1.0)]
    score_scale: f64,

    /// Stability threshold for 'keep' decision.
    #[arg(long, default_value_t = 0.85)]
    keep_threshold: f64,

    /// Stability threshold for 'drop' decision.
    #[arg(long, default_value_t = 0.40)]
    drop_threshold: f64,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Score(args) => run_score(args),
        Commands::Filter(args) => run_filter(args),
    }
}

fn run_score(args: ScoreArgs) -> Result<()> {
    let raw = read_input(&args.input)?;
    let observations = match args.format {
        Format::Jsonl => parse_jsonl(&raw).context("parsing JSONL")?,
        Format::Csv => parse_csv(raw.as_bytes()).context("parsing CSV")?,
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
    };
    let reports = score_all(observations, &config);
    let mut out = open_output(args.output.as_ref())?;
    for report in &reports {
        let line = serde_json::to_string(report).context("serializing report")?;
        writeln!(out, "{}", line)?;
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
        let report: quietset::StabilityReport = serde_json::from_str(line)
            .with_context(|| format!("parsing JSONL at line {}", i + 1))?;
        if let Some(min) = args.min_stability {
            if report.stability_score < min {
                continue;
            }
        }
        if let Some(max) = args.max_disagreement {
            if report.disagreement_score > max {
                continue;
            }
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
        writeln!(out, "{}", line)?;
    }
    Ok(())
}
