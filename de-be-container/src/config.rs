use std::net::IpAddr;
use std::path::Path;

use figment::Figment;
use figment::providers::{Env, Format, Yaml};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Error, PartialEq)]
pub enum ServiceConfigError {
    #[error("service.name must not be empty")]
    EmptyName,
    #[error("service.version must not be empty")]
    EmptyVersion,
    #[error("service.host must not be empty")]
    EmptyHost,
    #[error("service.host must be a valid IP address (got '{0}')")]
    InvalidHost(String),
    #[error("service.port must be greater than 0")]
    InvalidPort,
    #[error("service.log_level must not be empty")]
    EmptyLogLevel,
    #[error("service.log_level is not a valid tracing EnvFilter: {0}")]
    InvalidLogLevel(String),
    #[error("service.api_prefix must start with '/', have length > 1, and not end with '/'")]
    InvalidApiPrefix,
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
    #[error("sandbox.backend 'namespace' is not yet implemented; use 'process'")]
    UnsupportedBackendNamespace,
    #[error("sandbox.python_binary must not be empty")]
    EmptyPythonBinary,
    #[error("sandbox.timeout_ms must be greater than 0")]
    ZeroTimeout,
    #[error("sandbox.max_code_bytes must be greater than 0")]
    ZeroMaxCodeBytes,
    #[error("sandbox.max_sandbox_rows must be greater than 0")]
    ZeroMaxSandboxRows,
    #[error("sandbox.max_plot_points must be greater than 0")]
    ZeroMaxPlotPoints,
    #[error("sandbox.shm_dir must not be empty")]
    EmptyShmDir,
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

    #[error(transparent)]
    Visuals(#[from] crate::visuals::config::VisualsConfigError),

    #[error(transparent)]
    Mcp(#[from] crate::mcp::McpError),
}

fn default_api_prefix() -> String {
    "/api/data-engine/v1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServiceConfig {
    pub name: String,
    pub version: String,
    pub host: String,
    pub port: u16,
    pub log_level: String,
    #[serde(default = "default_api_prefix")]
    pub api_prefix: String,
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
        if self.host.parse::<IpAddr>().is_err() {
            return Err(ServiceConfigError::InvalidHost(self.host.clone()));
        }
        if self.port == 0 {
            return Err(ServiceConfigError::InvalidPort);
        }
        if self.log_level.trim().is_empty() {
            return Err(ServiceConfigError::EmptyLogLevel);
        }
        EnvFilter::try_new(&self.log_level)
            .map_err(|err| ServiceConfigError::InvalidLogLevel(err.to_string()))?;
        let prefix = self.api_prefix.trim();
        if !prefix.starts_with('/') || prefix.len() <= 1 || prefix.ends_with('/') {
            return Err(ServiceConfigError::InvalidApiPrefix);
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackend {
    Process,
    Namespace,
}

fn default_sandbox_backend() -> SandboxBackend {
    SandboxBackend::Process
}

fn default_max_sandbox_rows() -> usize {
    100_000
}

fn default_max_plot_points() -> usize {
    1000
}

fn default_shm_dir() -> String {
    "/dev/shm".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandboxConfig {
    #[serde(default = "default_sandbox_backend")]
    pub backend: SandboxBackend,
    pub python_binary: String,
    pub timeout_ms: u64,
    pub max_code_bytes: usize,
    #[serde(default = "default_max_sandbox_rows")]
    pub max_sandbox_rows: usize,
    #[serde(default = "default_max_plot_points")]
    pub max_plot_points: usize,
    #[serde(default = "default_shm_dir")]
    pub shm_dir: String,
    pub allowed_modules: Vec<String>,
}

impl SandboxConfig {
    pub fn validate(&self) -> Result<(), SandboxConfigError> {
        if self.backend == SandboxBackend::Namespace {
            return Err(SandboxConfigError::UnsupportedBackendNamespace);
        }
        if self.python_binary.trim().is_empty() {
            return Err(SandboxConfigError::EmptyPythonBinary);
        }
        if self.timeout_ms == 0 {
            return Err(SandboxConfigError::ZeroTimeout);
        }
        if self.max_code_bytes == 0 {
            return Err(SandboxConfigError::ZeroMaxCodeBytes);
        }
        if self.max_sandbox_rows == 0 {
            return Err(SandboxConfigError::ZeroMaxSandboxRows);
        }
        if self.max_plot_points == 0 {
            return Err(SandboxConfigError::ZeroMaxPlotPoints);
        }
        if self.shm_dir.trim().is_empty() {
            return Err(SandboxConfigError::EmptyShmDir);
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
    #[serde(default)]
    pub visuals: crate::visuals::config::VisualsConfig,
    #[serde(default)]
    pub mcp: crate::mcp::McpConfig,
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
        self.visuals.validate()?;
        self.mcp.validate()?;
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
  backend: "process"
  python_binary: "python3"
  timeout_ms: 15000
  max_code_bytes: 65536
  max_sandbox_rows: 100000
  max_plot_points: 1000
  shm_dir: "/dev/shm"
  allowed_modules:
    - "math"
    - "statistics"
    - "json"
    - "numpy"
    - "scipy"
    - "statsmodels"
    - "pyarrow"
"#
    }

    #[test]
    fn test_load_valid_config_from_yaml() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");

        let cfg = AppConfig::load(tmp.path()).expect("valid config should load");
        assert_eq!(cfg.service.name, "data-engine-be");
        assert_eq!(cfg.service.port, 8080);
        assert_eq!(cfg.service.api_prefix, "/api/data-engine/v1");
        assert_eq!(cfg.profiler.minhash_num_hashes, 64);
        assert_eq!(cfg.compute.join_inclusion_threshold, 0.80);
        assert_eq!(cfg.seeder.default_seed, 42);
        assert!(!cfg.sandbox.python_binary.is_empty());
        assert_eq!(cfg.sandbox.backend, SandboxBackend::Process);
        assert_eq!(cfg.sandbox.max_sandbox_rows, 100_000);
        assert_eq!(cfg.sandbox.max_plot_points, 1000);
    }

    #[test]
    fn test_service_config_api_prefix_validation() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");
        let mut cfg = AppConfig::load(tmp.path()).expect("load base");

        cfg.service.api_prefix = "/api/v1".to_string();
        assert!(cfg.validate().is_ok());

        cfg.service.api_prefix = "/api/custom/v1".to_string();
        assert!(cfg.validate().is_ok());

        cfg.service.api_prefix = "api/v1".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidApiPrefix))
        ));

        cfg.service.api_prefix = "/".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidApiPrefix))
        ));

        cfg.service.api_prefix = "/api/data-engine/v1/".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidApiPrefix))
        ));

        cfg.service.api_prefix = "".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidApiPrefix))
        ));
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

    #[test]
    fn test_sandbox_config_validation_rejects_zero_row_limit_or_empty_shm_dir() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");
        let mut cfg = AppConfig::load(tmp.path()).expect("load base");

        cfg.sandbox.max_sandbox_rows = 0;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Sandbox(SandboxConfigError::ZeroMaxSandboxRows))
        ));

        cfg.sandbox.max_sandbox_rows = 10_000;
        cfg.sandbox.max_plot_points = 0;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Sandbox(SandboxConfigError::ZeroMaxPlotPoints))
        ));

        cfg.sandbox.max_plot_points = 500;
        cfg.sandbox.shm_dir = "   ".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Sandbox(SandboxConfigError::EmptyShmDir))
        ));
    }

    #[test]
    fn test_mcp_config_validation_delegation() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");
        let mut cfg = AppConfig::load(tmp.path()).expect("load base");

        assert!(cfg.mcp.enabled);
        assert_eq!(cfg.mcp.server_name, "data-engine-mcp");

        cfg.mcp.server_name = "   ".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Mcp(crate::mcp::McpError::EmptyServerName))
        ));

        cfg.mcp.server_name = "data-engine-mcp".to_string();
        cfg.mcp.instructions = "".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Mcp(crate::mcp::McpError::EmptyInstructions))
        ));

        cfg.mcp.enabled = false;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_sandbox_config_rejects_namespace_backend() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");
        let mut cfg = AppConfig::load(tmp.path()).expect("load base");

        cfg.sandbox.backend = SandboxBackend::Namespace;
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Sandbox(
                SandboxConfigError::UnsupportedBackendNamespace
            ))
        ));
    }

    #[test]
    fn test_service_config_rejects_invalid_log_level() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");
        let mut cfg = AppConfig::load(tmp.path()).expect("load base");

        cfg.service.log_level = "de_be=debug,tower_http=info".to_string();
        assert!(cfg.validate().is_ok());

        cfg.service.log_level = "   ".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::EmptyLogLevel))
        ));

        cfg.service.log_level = "de_be[unclosed".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidLogLevel(_)))
        ));
    }

    #[test]
    fn test_service_config_rejects_invalid_host() {
        let mut tmp = NamedTempFile::new().expect("create temp yaml");
        tmp.write_all(sample_yaml().as_bytes()).expect("write yaml");
        let mut cfg = AppConfig::load(tmp.path()).expect("load base");

        cfg.service.host = "0.0.0.0".to_string();
        assert!(cfg.validate().is_ok());

        cfg.service.host = "::1".to_string();
        assert!(cfg.validate().is_ok());

        cfg.service.host = "   ".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::EmptyHost))
        ));

        cfg.service.host = "not-a-valid-ip".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidHost(ref h))) if h == "not-a-valid-ip"
        ));

        cfg.service.host = "999.999.999.999".to_string();
        assert!(matches!(
            cfg.validate(),
            Err(ConfigError::Service(ServiceConfigError::InvalidHost(_)))
        ));
    }
}
