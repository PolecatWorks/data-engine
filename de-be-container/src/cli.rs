use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser, PartialEq)]
#[command(
    name = "de-be",
    about = "PolecatWorks Data-Engine Backend (Connectors, Profiler, Join Probe & Chaos Seeder)"
)]
pub struct Cli {
    /// Path to YAML configuration file
    #[arg(short = 'c', long = "config", default_value = "config/default.yaml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand, PartialEq)]
pub enum Commands {
    /// Start the Axum HTTP API server
    Start,
    /// Validate configuration and exit
    Preflight,
    /// Generate the deterministic Chaos & Drift Validation Parquet Suite (TRAP-01..04)
    SeedValidationSuite(SeedValidationSuiteArgs),
    /// Statistically profile a Parquet dataset (and optionally compare against a baseline profile JSON)
    Profile(ProfileArgs),
    /// Run an empirical join probe across two Parquet datasets
    JoinProbe(JoinProbeArgs),
}

#[derive(Debug, Args, PartialEq)]
pub struct SeedValidationSuiteArgs {
    /// Output directory where Parquet tables and ground_truth_manifest.json are written
    #[arg(long = "output-dir")]
    pub output_dir: PathBuf,

    /// Optional deterministic RNG seed (defaults to seeder.default_seed from config)
    #[arg(long = "seed")]
    pub seed: Option<u64>,

    /// Optional base row count per primary table (defaults to seeder.default_row_count from config)
    #[arg(long = "row-count")]
    pub row_count: Option<usize>,
}

#[derive(Debug, Args, PartialEq)]
pub struct ProfileArgs {
    /// Logical dataset name
    #[arg(long = "dataset-name")]
    pub dataset_name: String,

    /// Path to Parquet file to profile
    #[arg(long = "parquet-path")]
    pub parquet_path: PathBuf,

    /// Optional path to baseline DatasetProfile JSON for temporal drift detection
    #[arg(long = "baseline-profile-json")]
    pub baseline_profile_json: Option<PathBuf>,
}

#[derive(Debug, Args, PartialEq)]
pub struct JoinProbeArgs {
    /// Left dataset logical name
    #[arg(long = "left-dataset")]
    pub left_dataset: String,
    /// Left dataset Parquet path
    #[arg(long = "left-parquet")]
    pub left_parquet: PathBuf,
    /// Left join column name
    #[arg(long = "left-field")]
    pub left_field: String,

    /// Right dataset logical name
    #[arg(long = "right-dataset")]
    pub right_dataset: String,
    /// Right dataset Parquet path
    #[arg(long = "right-parquet")]
    pub right_parquet: PathBuf,
    /// Right join column name
    #[arg(long = "right-field")]
    pub right_field: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_subcommands() {
        let cli = Cli::try_parse_from([
            "de-be",
            "-c",
            "de-be-container/config/default.yaml",
            "seed-validation-suite",
            "--output-dir",
            "/tmp/suite",
            "--seed",
            "99",
        ])
        .expect("parse seed-validation-suite");

        assert_eq!(
            cli.config,
            PathBuf::from("de-be-container/config/default.yaml")
        );
        assert_eq!(
            cli.command,
            Commands::SeedValidationSuite(SeedValidationSuiteArgs {
                output_dir: PathBuf::from("/tmp/suite"),
                seed: Some(99),
                row_count: None,
            })
        );

        let cli_preflight = Cli::try_parse_from(["de-be", "preflight"]).expect("parse preflight");
        assert_eq!(cli_preflight.command, Commands::Preflight);
    }
}
