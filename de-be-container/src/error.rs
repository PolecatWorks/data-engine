use thiserror::Error;

use crate::benchmark::BenchmarkError;
use crate::config::ConfigError;
use crate::join_probe::JoinProbeError;
use crate::profiler::ProfilerError;
use crate::provenance::ProvenanceError;
use crate::query_engine::QueryEngineError;
use crate::seeder::SeederError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Seeder(#[from] SeederError),

    #[error(transparent)]
    Profiler(#[from] ProfilerError),

    #[error(transparent)]
    JoinProbe(#[from] JoinProbeError),

    #[error(transparent)]
    Provenance(#[from] ProvenanceError),

    #[error(transparent)]
    QueryEngine(#[from] QueryEngineError),

    #[error(transparent)]
    Benchmark(#[from] BenchmarkError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
