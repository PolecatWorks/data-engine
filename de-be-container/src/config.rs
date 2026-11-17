use std::path::Path;

use figment::Figment;
use figment::providers::{Env, Format, Yaml};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ServiceConfigError {
    #[error("service.name must not be empty")]
    EmptyName,
    #[error("service.version must not be empty")]
    EmptyVersion,
    #[error("service.host must not be empty")]
    EmptyHost,
    #[error("service.port must be greater than 0")]
    InvalidPort,
    #[error("service.log_level must not be empty")]
    EmptyLogLevel,
}

#[derive(Debug, Error, PartialEq)]
pub enum ProfilerConfigError {
    #[error("profiler.max_sample_rows must be greater than 0")]
    ZeroMaxSampleRows,
    #[error("profiler.top_k_values must be greater than 0")]
    ZeroTopKValues,
    #[error("profiler.minhash_num_hashes must be greater than 0")]
    ZeroMinHashCount,
    #[error("profiler.drift_relative_mean_threshold must be greater than 1.0")]
    InvalidDriftThreshold,
}

#[derive(Debug, Error, PartialEq)]
pub enum ComputeConfigError {
    #[error("compute.query_timeout_secs must be greater than 0")]
    ZeroQueryTimeout,
    #[error("compute.max_preview_rows must be greater than 0")]
    ZeroMaxPreviewRows,
    #[error("compute.join_inclusion_threshold must be in (0.0, 1.0]")]
    InvalidJoinInclusionThreshold,
}

#[derive(Debug, Error, PartialEq)]
pub enum SeederConfigError {
    #[error("seeder.default_row_count must be at least 10")]
    RowCountTooSmall,
}

#[derive(Debug, Error, PartialEq)]
pub enum SandboxConfigError {
    #[error("sandbox.python_binary must not be empty")]
    EmptyPythonBinary,
    #[error("sandbox.timeout_ms must be greater than 0")]
    ZeroTimeout,
    #[error("sandbox.max_code_bytes must be greater than 0")]
    ZeroMaxCodeBytes,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(#[from] Box<figment::Error>),

    #[error(transparent)]
    Service(#[from] ServiceConfigError),

    #[error(transparent)]
    Profiler(#[from] ProfilerConfigError),

    #[error(transparent)]
    Compute(#[from] ComputeConfigError),

    #[error(transparent)]
    Seeder(#[from] SeederConfigError),

    #[error(transparent)]
    Sandbox(#[from] SandboxConfigError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceConfig {
    pub name: String,
    pub version: String,
    pub host: String,
    pub port: u16,
    pub log_level: String,
}

impl ServiceConfig {
    pub fn validate(&self) -> Result<(), ServiceConfigError> {
        if self.name.trim().is_empty() {
            return Err(ServiceConfigError::EmptyName);
        }
        if self.version.trim().is_empty() {
            return Err(ServiceConfigError::EmptyVersion);
        }
        if self.host.trim().is_empty() {
            return Err(ServiceConfigError::EmptyHost);
        }
        if self.port == 0 {
            return Err(ServiceConfigError::InvalidPort);
        }
        if self.log_level.trim().is_empty() {
            return Err(ServiceConfigError::EmptyLogLevel);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilerConfig {
    pub max_sample_rows: usize,
    pub top_k_values: usize,
    pub minhash_num_hashes: usize,
    pub drift_relative_mean_threshold: f64,
}

impl ProfilerConfig {
    pub fn validate(&self) -> Result<(), ProfilerConfigError> {
        if self.max_sample_rows == 0 {
            return Err(ProfilerConfigError::ZeroMaxSampleRows);
        }
        if self.top_k_values == 0 {
            return Err(ProfilerConfigError::ZeroTopKValues);
        }
        if self.minhash_num_hashes == 0 {
            return Err(ProfilerConfigError::ZeroMinHashCount);
        }
        if self.drift_relative_mean_threshold <= 1.0 {
            return Err(ProfilerConfigError::InvalidDriftThreshold);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComputeConfig {
    pub query_timeout_secs: u64,
    pub max_preview_rows: usize,
    pub join_inclusion_threshold: f64,
}

impl ComputeConfig {
    pub fn validate(&self) -> Result<(), ComputeConfigError> {
        if self.query_timeout_secs == 0 {
            return Err(ComputeConfigError::ZeroQueryTimeout);
        }
        if self.max_preview_rows == 0 {
            return Err(ComputeConfigError::ZeroMaxPreviewRows);
        }
        if self.join_inclusion_threshold <= 0.0 || self.join_inclusion_threshold > 1.0 {
            return Err(ComputeConfigError::InvalidJoinInclusionThreshold);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeederConfig {
    pub default_seed: u64,
    pub default_row_count: usize,
}

impl SeederConfig {
    pub fn validate(&self) -> Result<(), SeederConfigError> {
        if self.default_row_count < 10 {
            return Err(SeederConfigError::RowCountTooSmall);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxConfig {
    pub python_binary: String,
    pub timeout_ms: u64,
    pub max_code_bytes: usize,
    pub allowed_modules: Vec<String>,
}

impl SandboxConfig {
    pub fn validate(&self) -> Result<(), SandboxConfigError> {
        if self.python_binary.trim().is_empty() {
            return Err(SandboxConfigError::EmptyPythonBinary);
        }
        if self.timeout_ms == 0 {
            return Err(SandboxConfigError::ZeroTimeout);
        }
        if self.max_code_bytes == 0 {
            return Err(SandboxConfigError::ZeroMaxCodeBytes);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub service: ServiceConfig,
    pub profiler: ProfilerConfig,
    pub compute: ComputeConfig,
    pub seeder: SeederConfig,
    pub sandbox: SandboxConfig,
}

impl AppConfig {
    pub fn load(config_path: &Path) -> Result<Self, ConfigError> {
        let cfg: Self = Figment::new()
            .merge(Yaml::file(config_path))
            .merge(Env::prefixed("DE_BE__").split("__"))
            .extract()
            .map_err(Box::new)?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.service.validate()?;
        self.profiler.validate()?;
        self.compute.validate()?;
        self.seeder.validate()?;
        self.sandbox.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_yaml() -> &'static str {
        r#"
service:
  name: "data-engine-be"
  version: "0.1.0"
  host: "127.0.0.1"
  port: 8080
  log_level: "info"
profiler:
  max_sample_rows: 50000
  top_k_values: 10
  minhash_num_hashes: 64
  drift_relative_mean_threshold: 5.0
compute:
  query_timeout_secs: 30
  max_preview_rows: 100
  join_inclusion_threshold: 0.80
seeder:
  default_seed: 42
  default_row_count: 200
sandbox:
  python_binary: "python3"
  timeout_ms: 5000
  max_code_bytes: 65536
  allowed_modules:
    - "math"
    - "statistics"
    - "json"
"#
    }

    #[test]
    fn test_load_valid_config_from_yaml() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");

        let cfg = AppConfig::load(tmp.path()).expect("valid config should load");
        assert_eq!(cfg.service.name, "data-engine-be");
        assert_eq!(cfg.service.port, 8080);
        assert_eq!(cfg.profiler.minhash_num_hashes, 64);
        assert_eq!(cfg.compute.join_inclusion_threshold, 0.80);
        assert_eq!(cfg.seeder.default_seed, 42);
        assert_eq!(cfg.sandbox.python_binary, "python3");
    }

    #[test]
    fn test_config_validation_fails_on_invalid_fields() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");
        let mut cfg = AppConfig::load(tmp.path()).expect("load base");

        cfg.service.name = "  ".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::EmptyName))
        ));

        cfg.service.name = "data-engine-be".to_string();
        cfg.service.port = 0;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidPort))
        ));

        cfg.service.port = 8080;
        cfg.profiler.max_sample_rows = 0;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Profiler(ProfilerConfigError::ZeroMaxSampleRows))
        ));

        cfg.profiler.max_sample_rows = 1000;
        cfg.compute.join_inclusion_threshold = 1.5;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Compute(
                ComputeConfigError::InvalidJoinInclusionThreshold
            ))
        ));

        cfg.compute.join_inclusion_threshold = 0.80;
        cfg.seeder.default_row_count = 3;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Seeder(SeederConfigError::RowCountTooSmall))
        ));

        cfg.seeder.default_row_count = 100;
        cfg.sandbox.timeout_ms = 0;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Sandbox(SandboxConfigError::ZeroTimeout))
        ));
    }
}
