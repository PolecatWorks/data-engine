use thiserror::Error;

use crate::config::ConfigError;
use crate::join_probe::JoinProbeError;
use crate::profiler::ProfilerError;
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

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
