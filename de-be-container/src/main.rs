pub mod analytics;
pub mod benchmark;
pub mod cli;
pub mod config;
pub mod error;
pub mod join_probe;
pub mod profiler;
pub mod provenance;
pub mod query_engine;
pub mod sandbox;
pub mod seeder;
pub mod web;

use std::fs;
use std::sync::Arc;

use clap::Parser;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::analytics::{BuiltinAlgorithmRegistry, FieldStatisticalCatalog};
use crate::benchmark::run_ablation_benchmark;
use crate::cli::{Cli, Commands};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::join_probe::probe_join;
use crate::profiler::{DatasetProfile, profile_parquet_dataset};
use crate::provenance::ProvenanceStore;
use crate::seeder::seed_validation_suite_with_options;
use crate::web::{AppState, build_router};

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let cli = Cli::parse();
    let config = AppConfig::load(&cli.config)?;

    let filter = EnvFilter::try_new(&config.service.log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();

    match cli.command {
        Commands::Preflight => {
            info!(
                service = %config.service.name,
                version = %config.service.version,
                "Preflight configuration check passed"
            );
            Ok(())
        }
        Commands::SeedValidationSuite(args) => {
            let seed = args.seed.unwrap_or(config.seeder.default_seed);
            let row_count = args.row_count.unwrap_or(config.seeder.default_row_count);
            let manifest = seed_validation_suite_with_options(
                seed,
                row_count,
                &args.output_dir,
                args.include_stat_traps,
            )?;
            let out = serde_json::to_string_pretty(&manifest)
                .map_err(crate::seeder::SeederError::from)?;
            println!("{}", out);
            Ok(())
        }
        Commands::HydrateBuiltins => {
            let registry = BuiltinAlgorithmRegistry::default();
            let hydrated = registry.hydrate_builtins(&config.sandbox)?;
            let out = serde_json::to_string_pretty(&hydrated)
                .map_err(crate::seeder::SeederError::from)?;
            println!("{}", out);
            Ok(())
        }
        Commands::Profile(args) => {
            let baseline: Option<DatasetProfile> =
                if let Some(base_path) = &args.baseline_profile_json {
                    let raw = fs::read_to_string(base_path)?;
                    let parsed: DatasetProfile = serde_json::from_str(&raw)
                        .map_err(crate::seeder::SeederError::from)?;
                    Some(parsed)
                } else {
                    None
                };
            let profile = profile_parquet_dataset(
                &args.dataset_name,
                &args.parquet_path,
                &config.profiler,
                baseline.as_ref(),
            )?;
            let out = serde_json::to_string_pretty(&profile)
                .map_err(crate::seeder::SeederError::from)?;
            println!("{}", out);
            Ok(())
        }
        Commands::JoinProbe(args) => {
            let res = probe_join(
                &args.left_dataset,
                &args.left_parquet,
                &args.left_field,
                &args.right_dataset,
                &args.right_parquet,
                &args.right_field,
                &config.compute,
            )?;
            let out =
                serde_json::to_string_pretty(&res).map_err(crate::seeder::SeederError::from)?;
            println!("{}", out);
            Ok(())
        }
        Commands::RunAblationBenchmark(args) => {
            let seed = args.seed.unwrap_or(config.seeder.default_seed);
            let row_count = args.row_count.unwrap_or(config.seeder.default_row_count);
            let provenance_store = ProvenanceStore::default();
            let scorecard = run_ablation_benchmark(
                seed,
                row_count,
                &args.output_dir,
                &config.profiler,
                &config.compute,
                &config.sandbox,
                &provenance_store,
            )?;
            let out = serde_json::to_string_pretty(&scorecard)
                .map_err(crate::seeder::SeederError::from)?;
            println!("{}", out);
            Ok(())
        }
        Commands::Start => {
            let bind_addr = format!("{}:{}", config.service.host, config.service.port);
            let algorithm_registry = BuiltinAlgorithmRegistry::default();
            let hydrated = algorithm_registry.hydrate_builtins(&config.sandbox)?;
            info!(
                %bind_addr,
                hydrated_builtins = hydrated.len(),
                "Starting data-engine backend HTTP service"
            );
            let state = AppState {
                config: Arc::new(config),
                provenance_store: ProvenanceStore::default(),
                algorithm_registry,
                field_catalog: FieldStatisticalCatalog::default(),
            };
            let router = build_router(state);
            let listener = TcpListener::bind(&bind_addr).await?;
            axum::serve(listener, router).await?;
            Ok(())
        }
    }
}
