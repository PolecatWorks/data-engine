//! Curated Phase 1 Statistical Analytics Toolbelt, Universal `StatisticalResultEnvelope`,
//! Built-In Versioned Algorithm Registry, and Field Statistical Profile Catalog (Specs 10 & 11).

pub mod scripts;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use arrow::array::RecordBatch;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::SandboxConfig;
use crate::provenance::{ProvenanceError, ProvenanceStore, TrustSummary};
use crate::sandbox::{
    ArrowSandboxJob, ProcessSandbox, SampleContext, SamplingSpec, SandboxError, SandboxExecutor,
    read_parquet_batches,
};

#[derive(Debug, Error)]
pub enum AnalyticsError {
    #[error(transparent)]
    Sandbox(#[from] SandboxError),

    #[error(transparent)]
    Provenance(#[from] ProvenanceError),

    #[error("invalid analytics request: {0}")]
    InvalidRequest(String),

    #[error("analytics catalog lock poisoned")]
    LockPoisoned,

    #[error("malformed analytics sandbox output for field '{field}': {source}")]
    MalformedSandboxOutput {
        field: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssumptionCheck {
    pub check: String,
    pub status: String,
    pub statistic: Option<f64>,
    pub p_value: Option<f64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecommendedAlternative {
    pub method: String,
    pub reason: String,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticPlotSpec {
    pub plot_type: String,
    pub title: String,
    pub series: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LearnedFieldTrait {
    pub field_canonical_id: String,
    #[serde(default)]
    pub distribution_family: Option<String>,
    #[serde(default)]
    pub is_heavy_tailed: bool,
    #[serde(default)]
    pub skewness: Option<f64>,
    #[serde(default)]
    pub kurtosis: Option<f64>,
    #[serde(default)]
    pub seasonality_period: Option<String>,
    #[serde(default)]
    pub recommended_test_family: Option<String>,
    #[serde(default)]
    pub known_confounders: Vec<String>,
    #[serde(default = "default_empirically_tested")]
    pub epistemic_status: String,
    #[serde(default = "default_conf")]
    pub confidence: f64,
    #[serde(default)]
    pub verified_by: Option<String>,
    #[serde(default)]
    pub last_diagnosed_run_id: Option<String>,
}

fn default_empirically_tested() -> String {
    "empirically_tested".to_string()
}

fn default_conf() -> f64 {
    0.90
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatisticalResultEnvelope {
    pub tool: String,
    pub algorithm_id: String,
    pub algorithm_version: u32,
    pub code_sha256: String,
    pub sample_context: SampleContext,
    pub primary_result: serde_json::Value,
    pub assumption_checks: Vec<AssumptionCheck>,
    pub recommended_alternative: Option<RecommendedAlternative>,
    pub diagnostic_plots: Vec<DiagnosticPlotSpec>,
    pub learned_field_traits: Vec<LearnedFieldTrait>,
    pub provenance_run_id: String,
    pub trust_summary: TrustSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BuiltinAlgorithmRecord {
    pub algorithm_id: String,
    pub mcp_tool_name: String,
    pub version: u32,
    pub description: String,
    pub code_sha256: String,
    pub validation_status: String,
}

#[derive(Clone, Default)]
pub struct BuiltinAlgorithmRegistry {
    records: Arc<RwLock<BTreeMap<String, BuiltinAlgorithmRecord>>>,
}

impl BuiltinAlgorithmRegistry {
    pub fn hydrate_builtins(
        &self,
        config: &SandboxConfig,
    ) -> Result<Vec<BuiltinAlgorithmRecord>, AnalyticsError> {
        let executor = ProcessSandbox::new(config.clone());
        let defs = [
            (
                "builtin_stat_describe_and_check",
                "stat_describe_and_check",
                "Distributional moments, quantiles, normality, outlier, and missingness diagnostics",
                scripts::BUILTIN_STAT_DESCRIBE_CODE,
            ),
            (
                "builtin_stat_hypothesis_test",
                "stat_hypothesis_test",
                "Parametric & non-parametric hypothesis testing with assumption checks, CIs, FDR, and Simpson's paradox detection",
                scripts::BUILTIN_STAT_HYPOTHESIS_CODE,
            ),
            (
                "builtin_stat_power_analysis",
                "stat_power_analysis",
                "Prospective & retrospective statistical power, sample-size, and MDE calculator",
                scripts::BUILTIN_STAT_POWER_CODE,
            ),
            (
                "builtin_stat_correlation_vif",
                "stat_correlation_vif",
                "Pearson/Spearman/Kendall matrices, partial correlations, FDR adjustment, and VIF multicollinearity diagnostics",
                scripts::BUILTIN_STAT_CORRELATION_VIF_CODE,
            ),
            (
                "builtin_stat_regression_fit",
                "stat_regression_fit",
                "OLS, GLM, and Huber robust regression with HC3 SEs, Breusch-Pagan, Durbin-Watson, and Cook's distance diagnostics",
                scripts::BUILTIN_STAT_REGRESSION_CODE,
            ),
            (
                "builtin_stat_timeseries_analyze",
                "stat_timeseries_analyze",
                "STL decomposition, ADF/KPSS stationarity, ACF/PACF, spurious correlation detection, and ETS/ARIMA forecasting",
                scripts::BUILTIN_STAT_TIMESERIES_CODE,
            ),
        ];

        let mut hydrated = Vec::with_capacity(defs.len());
        let mut guard = self
            .records
            .write()
            .map_err(|_| AnalyticsError::LockPoisoned)?;

        for (alg_id, tool_name, desc, code) in defs {
            let validation = executor.validate_code(alg_id, desc, code, "run")?;
            if !validation.is_valid {
                return Err(AnalyticsError::Sandbox(SandboxError::SecurityViolation(
                    validation.violations,
                )));
            }
            let rec = BuiltinAlgorithmRecord {
                algorithm_id: alg_id.to_string(),
                mcp_tool_name: tool_name.to_string(),
                version: 1,
                description: desc.to_string(),
                code_sha256: validation.code_sha256,
                validation_status: "validated".to_string(),
            };
            guard.insert(alg_id.to_string(), rec.clone());
            hydrated.push(rec);
        }

        Ok(hydrated)
    }

    pub fn list(&self) -> Result<Vec<BuiltinAlgorithmRecord>, AnalyticsError> {
        let guard = self
            .records
            .read()
            .map_err(|_| AnalyticsError::LockPoisoned)?;
        Ok(guard.values().cloned().collect())
    }
}

#[derive(Clone, Default)]
pub struct FieldStatisticalCatalog {
    traits: Arc<RwLock<BTreeMap<String, LearnedFieldTrait>>>,
}

impl FieldStatisticalCatalog {
    pub fn merge_traits(
        &self,
        incoming: &[LearnedFieldTrait],
        run_id: &str,
    ) -> Result<(), AnalyticsError> {
        let mut guard = self
            .traits
            .write()
            .map_err(|_| AnalyticsError::LockPoisoned)?;
        for item in incoming {
            let mut next = item.clone();
            next.last_diagnosed_run_id = Some(run_id.to_string());
            if let Some(existing) = guard.get(&next.field_canonical_id) {
                if existing.epistemic_status == "verified" && next.epistemic_status != "drifted" {
                    let mut preserved = existing.clone();
                    preserved.last_diagnosed_run_id = Some(run_id.to_string());
                    guard.insert(preserved.field_canonical_id.clone(), preserved);
                    continue;
                }
            }
            guard.insert(next.field_canonical_id.clone(), next);
        }
        Ok(())
    }

    pub fn verify_trait(
        &self,
        field_canonical_id: &str,
        verified_by: &str,
        recommended_test_family: Option<String>,
        known_confounders: Option<Vec<String>>,
    ) -> Result<Option<LearnedFieldTrait>, AnalyticsError> {
        let mut guard = self
            .traits
            .write()
            .map_err(|_| AnalyticsError::LockPoisoned)?;
        let Some(existing) = guard.get_mut(field_canonical_id) else {
            return Ok(None);
        };
        existing.epistemic_status = "verified".to_string();
        existing.verified_by = Some(verified_by.to_string());
        existing.confidence = 1.0;
        if let Some(rec) = recommended_test_family {
            existing.recommended_test_family = Some(rec);
        }
        if let Some(conf) = known_confounders {
            existing.known_confounders = conf;
        }
        Ok(Some(existing.clone()))
    }

    pub fn get(&self, field_canonical_id: &str) -> Result<Option<LearnedFieldTrait>, AnalyticsError> {
        let guard = self
            .traits
            .read()
            .map_err(|_| AnalyticsError::LockPoisoned)?;
        Ok(guard.get(field_canonical_id).cloned())
    }

    pub fn list(&self) -> Result<Vec<LearnedFieldTrait>, AnalyticsError> {
        let guard = self
            .traits
            .read()
            .map_err(|_| AnalyticsError::LockPoisoned)?;
        Ok(guard.values().cloned().collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputBinding {
    #[serde(default = "default_dataset_name")]
    pub dataset_name: String,
    pub parquet_path: String,
}

fn default_dataset_name() -> String {
    "dataset".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatDescribeRequest {
    pub input_binding: InputBinding,
    pub columns: Vec<String>,
    #[serde(default)]
    pub sampling: Option<SamplingSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatHypothesisTestRequest {
    pub input_binding: InputBinding,
    pub method: String,
    pub value_col: String,
    #[serde(default)]
    pub group_col: Option<String>,
    #[serde(default)]
    pub col_a: Option<String>,
    #[serde(default)]
    pub col_b: Option<String>,
    #[serde(default = "default_alpha")]
    pub alpha: f64,
    #[serde(default = "default_alt")]
    pub alternative: String,
    #[serde(default = "default_correction")]
    pub correction: String,
    #[serde(default)]
    pub segment_col: Option<String>,
    #[serde(default)]
    pub stratify_by: Option<String>,
    #[serde(default)]
    pub sampling: Option<SamplingSpec>,
}

fn default_alpha() -> f64 {
    0.05
}
fn default_alt() -> String {
    "two-sided".to_string()
}
fn default_correction() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatPowerAnalysisRequest {
    pub test_type: String,
    pub solve_for: String,
    #[serde(default = "default_alpha")]
    pub alpha: f64,
    #[serde(default)]
    pub power: Option<f64>,
    #[serde(default)]
    pub effect_size: Option<f64>,
    #[serde(default)]
    pub p1: Option<f64>,
    #[serde(default)]
    pub p2: Option<f64>,
    #[serde(default)]
    pub n_per_group: Option<usize>,
    #[serde(default = "default_ratio")]
    pub ratio: f64,
    #[serde(default = "default_num_groups")]
    pub num_groups: usize,
    #[serde(default = "default_alt")]
    pub alternative: String,
}

fn default_ratio() -> f64 {
    1.0
}
fn default_num_groups() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatCorrelationVifRequest {
    pub input_binding: InputBinding,
    pub columns: Vec<String>,
    #[serde(default = "default_corr_methods")]
    pub methods: Vec<String>,
    #[serde(default)]
    pub control_cols: Vec<String>,
    #[serde(default = "default_fdr")]
    pub correction: String,
    #[serde(default)]
    pub sampling: Option<SamplingSpec>,
}

fn default_corr_methods() -> Vec<String> {
    vec!["pearson".to_string(), "spearman".to_string()]
}
fn default_fdr() -> String {
    "fdr_bh".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatRegressionFitRequest {
    pub input_binding: InputBinding,
    pub model_type: String,
    pub target_col: String,
    pub feature_cols: Vec<String>,
    #[serde(default = "default_true")]
    pub add_intercept: bool,
    #[serde(default)]
    pub sampling: Option<SamplingSpec>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatTimeseriesAnalyzeRequest {
    pub input_binding: InputBinding,
    pub time_col: String,
    pub value_col: String,
    #[serde(default)]
    pub compare_col: Option<String>,
    #[serde(default)]
    pub period: Option<usize>,
    #[serde(default)]
    pub forecast_horizon: Option<usize>,
    #[serde(default = "default_ts_model")]
    pub model: String,
    #[serde(default = "default_anom_z")]
    pub anomaly_z_threshold: f64,
    #[serde(default)]
    pub sampling: Option<SamplingSpec>,
}

fn default_ts_model() -> String {
    "ets".to_string()
}
fn default_anom_z() -> f64 {
    3.0
}

fn execute_builtin_stat_batches(
    tool_name: &str,
    algorithm_id: &str,
    code: &str,
    dataset_name: &str,
    batches: Vec<RecordBatch>,
    params: serde_json::Value,
    sampling: Option<SamplingSpec>,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<StatisticalResultEnvelope, AnalyticsError> {
    let executor = ProcessSandbox::new(config.clone());
    let mut input_batches = BTreeMap::new();
    if !batches.is_empty() {
        input_batches.insert("data".to_string(), batches);
    }

    let raw_out = executor.execute_arrow(ArrowSandboxJob {
        algorithm_id: algorithm_id.to_string(),
        code: code.to_string(),
        input_batches,
        params,
        sampling,
    })?;

    let mut sample_context = raw_out.sample_context;
    if let Some(gs) = raw_out.output.get("group_sizes").and_then(|v| v.as_object()) {
        for (k, v) in gs {
            if let Some(n) = v.as_u64() {
                sample_context.group_sizes.insert(k.clone(), n as usize);
            }
        }
    }

    let primary_result = raw_out
        .output
        .get("primary_result")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let assumption_checks: Vec<AssumptionCheck> = match raw_out.output.get("assumption_checks") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|source| {
            AnalyticsError::MalformedSandboxOutput {
                field: "assumption_checks",
                source,
            }
        })?,
        _ => Vec::new(),
    };
    let recommended_alternative: Option<RecommendedAlternative> =
        match raw_out.output.get("recommended_alternative") {
            Some(v) if !v.is_null() => {
                Some(serde_json::from_value(v.clone()).map_err(|source| {
                    AnalyticsError::MalformedSandboxOutput {
                        field: "recommended_alternative",
                        source,
                    }
                })?)
            }
            _ => None,
        };
    let diagnostic_plots: Vec<DiagnosticPlotSpec> = match raw_out.output.get("diagnostic_plots") {
        Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|source| {
            AnalyticsError::MalformedSandboxOutput {
                field: "diagnostic_plots",
                source,
            }
        })?,
        _ => Vec::new(),
    };
    let learned_field_traits: Vec<LearnedFieldTrait> =
        match raw_out.output.get("learned_field_traits") {
            Some(v) if !v.is_null() => serde_json::from_value(v.clone()).map_err(|source| {
                AnalyticsError::MalformedSandboxOutput {
                    field: "learned_field_traits",
                    source,
                }
            })?,
            _ => Vec::new(),
        };

    let mut rules_applied = vec![
        format!("algorithm_id={}", algorithm_id),
        format!("code_sha256={}", raw_out.code_sha256),
        format!("sampled={}", sample_context.sampled),
    ];
    if let Some(seed) = sample_context.sampling_seed {
        rules_applied.push(format!("sampling_seed={}", seed));
    }
    let trust_summary = TrustSummary {
        all_rules_human_verified: true,
        min_rule_confidence: 1.0,
        verified_by_stewards: vec!["data-engine-builtin".to_string()],
        pre_aggregation_applied: false,
        unit_conversion_applied: false,
    };

    let scalar_metric = primary_result
        .get("p_value")
        .or_else(|| primary_result.get("r_squared"))
        .or_else(|| primary_result.get("required_n_per_group"))
        .and_then(|v| v.as_f64())
        .unwrap_or(sample_context.analyzed_rows as f64);

    let run = provenance_store.record_run(
        tool_name,
        vec![dataset_name.to_string()],
        rules_applied,
        scalar_metric,
        trust_summary.clone(),
    )?;

    field_catalog.merge_traits(&learned_field_traits, &run.provenance_run_id)?;

    Ok(StatisticalResultEnvelope {
        tool: tool_name.to_string(),
        algorithm_id: algorithm_id.to_string(),
        algorithm_version: 1,
        code_sha256: raw_out.code_sha256,
        sample_context,
        primary_result,
        assumption_checks,
        recommended_alternative,
        diagnostic_plots,
        learned_field_traits,
        provenance_run_id: run.provenance_run_id,
        trust_summary,
    })
}

pub fn run_stat_describe(
    req: &StatDescribeRequest,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<StatisticalResultEnvelope, AnalyticsError> {
    if req.columns.is_empty() {
        return Err(AnalyticsError::InvalidRequest(
            "columns must not be empty".to_string(),
        ));
    }
    let batches = read_parquet_batches(Path::new(&req.input_binding.parquet_path))?;
    let params = serde_json::json!({
        "columns": req.columns,
        "dataset_name": req.input_binding.dataset_name,
        "max_plot_points": config.max_plot_points,
    });
    execute_builtin_stat_batches(
        "stat_describe_and_check",
        "builtin_stat_describe_and_check",
        scripts::BUILTIN_STAT_DESCRIBE_CODE,
        &req.input_binding.dataset_name,
        batches,
        params,
        req.sampling.clone(),
        config,
        provenance_store,
        field_catalog,
    )
}

pub fn run_stat_hypothesis_test(
    req: &StatHypothesisTestRequest,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<StatisticalResultEnvelope, AnalyticsError> {
    let batches = read_parquet_batches(Path::new(&req.input_binding.parquet_path))?;
    let params = serde_json::json!({
        "method": req.method,
        "value_col": req.value_col,
        "group_col": req.group_col,
        "col_a": req.col_a,
        "col_b": req.col_b,
        "alpha": req.alpha,
        "alternative": req.alternative,
        "correction": req.correction,
        "segment_col": req.segment_col,
        "stratify_by": req.stratify_by,
        "dataset_name": req.input_binding.dataset_name,
        "max_plot_points": config.max_plot_points,
    });
    execute_builtin_stat_batches(
        "stat_hypothesis_test",
        "builtin_stat_hypothesis_test",
        scripts::BUILTIN_STAT_HYPOTHESIS_CODE,
        &req.input_binding.dataset_name,
        batches,
        params,
        req.sampling.clone(),
        config,
        provenance_store,
        field_catalog,
    )
}

pub fn run_stat_power_analysis(
    req: &StatPowerAnalysisRequest,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<StatisticalResultEnvelope, AnalyticsError> {
    let params = serde_json::json!({
        "test_type": req.test_type,
        "solve_for": req.solve_for,
        "alpha": req.alpha,
        "power": req.power,
        "effect_size": req.effect_size,
        "p1": req.p1,
        "p2": req.p2,
        "n_per_group": req.n_per_group,
        "ratio": req.ratio,
        "num_groups": req.num_groups,
        "alternative": req.alternative,
    });
    execute_builtin_stat_batches(
        "stat_power_analysis",
        "builtin_stat_power_analysis",
        scripts::BUILTIN_STAT_POWER_CODE,
        "power_analysis_analytical",
        Vec::new(),
        params,
        None,
        config,
        provenance_store,
        field_catalog,
    )
}

pub fn run_stat_correlation_vif(
    req: &StatCorrelationVifRequest,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<StatisticalResultEnvelope, AnalyticsError> {
    if req.columns.len() < 2 {
        return Err(AnalyticsError::InvalidRequest(
            "correlation_vif requires at least 2 columns".to_string(),
        ));
    }
    let batches = read_parquet_batches(Path::new(&req.input_binding.parquet_path))?;
    let params = serde_json::json!({
        "columns": req.columns,
        "methods": req.methods,
        "control_cols": req.control_cols,
        "correction": req.correction,
    });
    execute_builtin_stat_batches(
        "stat_correlation_vif",
        "builtin_stat_correlation_vif",
        scripts::BUILTIN_STAT_CORRELATION_VIF_CODE,
        &req.input_binding.dataset_name,
        batches,
        params,
        req.sampling.clone(),
        config,
        provenance_store,
        field_catalog,
    )
}

pub fn run_stat_regression_fit(
    req: &StatRegressionFitRequest,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<StatisticalResultEnvelope, AnalyticsError> {
    if req.feature_cols.is_empty() {
        return Err(AnalyticsError::InvalidRequest(
            "feature_cols must not be empty".to_string(),
        ));
    }
    let batches = read_parquet_batches(Path::new(&req.input_binding.parquet_path))?;
    let params = serde_json::json!({
        "model_type": req.model_type,
        "target_col": req.target_col,
        "feature_cols": req.feature_cols,
        "add_intercept": req.add_intercept,
        "max_plot_points": config.max_plot_points,
    });
    execute_builtin_stat_batches(
        "stat_regression_fit",
        "builtin_stat_regression_fit",
        scripts::BUILTIN_STAT_REGRESSION_CODE,
        &req.input_binding.dataset_name,
        batches,
        params,
        req.sampling.clone(),
        config,
        provenance_store,
        field_catalog,
    )
}

pub fn run_stat_timeseries_analyze(
    req: &StatTimeseriesAnalyzeRequest,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<StatisticalResultEnvelope, AnalyticsError> {
    let batches = read_parquet_batches(Path::new(&req.input_binding.parquet_path))?;
    let params = serde_json::json!({
        "time_col": req.time_col,
        "value_col": req.value_col,
        "compare_col": req.compare_col,
        "period": req.period,
        "forecast_horizon": req.forecast_horizon,
        "model": req.model,
        "anomaly_z_threshold": req.anomaly_z_threshold,
        "dataset_name": req.input_binding.dataset_name,
        "max_plot_points": config.max_plot_points,
    });
    execute_builtin_stat_batches(
        "stat_timeseries_analyze",
        "builtin_stat_timeseries_analyze",
        scripts::BUILTIN_STAT_TIMESERIES_CODE,
        &req.input_binding.dataset_name,
        batches,
        params,
        req.sampling.clone(),
        config,
        provenance_store,
        field_catalog,
    )
}

pub fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SandboxBackend;
    use crate::sandbox::detect_test_python_binary;
    use arrow::array::{Float64Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;
    use std::fs::File;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn sci_config() -> SandboxConfig {
        SandboxConfig {
            backend: SandboxBackend::Process,
            python_binary: detect_test_python_binary(),
            timeout_ms: 20_000,
            max_code_bytes: 65_536,
            max_sandbox_rows: 100_000,
            max_plot_points: 1000,
            shm_dir: if Path::new("/dev/shm").exists() {
                PathBuf::from("/dev/shm")
            } else {
                std::env::temp_dir()
            },
            allowed_modules: vec![
                "math".to_string(),
                "statistics".to_string(),
                "json".to_string(),
                "numpy".to_string(),
                "scipy".to_string(),
                "statsmodels".to_string(),
                "pyarrow".to_string(),
            ],
        }
    }

    fn write_parquet(path: &Path, batch: &RecordBatch) {
        let f = File::create(path).expect("create parquet");
        let mut w = ArrowWriter::try_new(f, batch.schema(), None).expect("writer");
        w.write(batch).expect("write");
        w.close().expect("close");
    }

    // NIST StRD Norris linear regression certified dataset (36 points)
    // Certified OLS: intercept = -0.262323073774029, slope = 1.00211681802045, R^2 = 0.999993745883712
    fn nist_norris_batch() -> RecordBatch {
        let y: Vec<f64> = vec![
            0.1, 338.8, 118.1, 888.0, 9.2, 228.1, 668.5, 998.5, 449.1, 778.9, 559.2, 0.3, 0.1,
            778.1, 668.8, 339.3, 448.9, 10.8, 557.7, 228.3, 998.0, 888.8, 119.6, 0.3, 0.6, 557.6,
            339.3, 888.0, 998.5, 778.9, 10.2, 117.6, 228.9, 668.4, 449.2, 0.2,
        ];
        let x: Vec<f64> = vec![
            0.2, 337.4, 118.2, 884.6, 10.1, 226.5, 666.3, 996.3, 448.6, 777.0, 558.2, 0.4, 0.6,
            775.5, 666.9, 338.0, 447.5, 11.6, 556.0, 228.1, 995.8, 887.6, 120.2, 0.3, 0.3, 556.8,
            339.1, 887.2, 999.0, 779.0, 11.1, 118.3, 229.2, 669.1, 448.9, 0.5,
        ];
        let schema = Arc::new(Schema::new(vec![
            Field::new("x", DataType::Float64, false),
            Field::new("y", DataType::Float64, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Float64Array::from(x)),
                Arc::new(Float64Array::from(y)),
            ],
        )
        .unwrap()
    }

    #[test]
    fn test_hydrate_builtins_registers_pinned_sha256_algorithms() {
        let cfg = sci_config();
        let reg = BuiltinAlgorithmRegistry::default();
        let first = reg.hydrate_builtins(&cfg).expect("first hydrate");
        let second = reg.hydrate_builtins(&cfg).expect("second hydrate");
        assert_eq!(first.len(), 6);
        assert_eq!(first, second);
        for rec in &first {
            assert_eq!(rec.validation_status, "validated");
            assert_eq!(rec.code_sha256.len(), 64);
        }
    }

    #[test]
    fn test_stat_describe_nist_norris_and_heavy_tail() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("norris_heavy.parquet");

        // Build heavy-tailed positive series + Norris y
        let norris = nist_norris_batch();
        let y_col = norris.column(1).clone();
        let mut heavy = vec![50.0_f64; 36];
        for (i, v) in heavy.iter_mut().enumerate() {
            *v = 20.0 + (i as f64 % 5.0) * 3.0;
        }
        heavy[0] = 50_000.0;
        heavy[1] = 45_000.0;

        let schema = Arc::new(Schema::new(vec![
            Field::new("y", DataType::Float64, false),
            Field::new("heavy_usd", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![y_col, Arc::new(Float64Array::from(heavy))],
        )
        .unwrap();
        write_parquet(&path, &batch);

        let cfg = sci_config();
        let prov = ProvenanceStore::default();
        let cat = FieldStatisticalCatalog::default();

        let env = run_stat_describe(
            &StatDescribeRequest {
                input_binding: InputBinding {
                    dataset_name: "norris_ds".to_string(),
                    parquet_path: path.to_string_lossy().to_string(),
                },
                columns: vec!["y".to_string(), "heavy_usd".to_string()],
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("run stat_describe");

        assert_eq!(env.tool, "stat_describe_and_check");
        assert!(!env.provenance_run_id.is_empty());
        let heavy_stats = &env.primary_result["columns"]["heavy_usd"];
        assert_eq!(heavy_stats["is_heavy_tailed"].as_bool(), Some(true));
        assert!(
            env.assumption_checks
                .iter()
                .any(|c| c.check == "normality_heavy_usd" && c.status == "failed")
        );
        let stored = cat.get("norris_ds.heavy_usd").unwrap().expect("trait stored");
        assert!(stored.is_heavy_tailed);
        assert_eq!(
            stored.recommended_test_family.as_deref(),
            Some("non_parametric_or_log_transform")
        );
    }

    #[test]
    fn test_stat_regression_fit_nist_norris_and_heteroscedasticity() {
        let dir = tempdir().unwrap();
        let norris_path = dir.path().join("norris.parquet");
        write_parquet(&norris_path, &nist_norris_batch());

        let cfg = sci_config();
        let prov = ProvenanceStore::default();
        let cat = FieldStatisticalCatalog::default();

        let env = run_stat_regression_fit(
            &StatRegressionFitRequest {
                input_binding: InputBinding {
                    dataset_name: "nist_norris".to_string(),
                    parquet_path: norris_path.to_string_lossy().to_string(),
                },
                model_type: "ols".to_string(),
                target_col: "y".to_string(),
                feature_cols: vec!["x".to_string()],
                add_intercept: true,
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("nist norris OLS");

        let coefs = env.primary_result["coefficients"].as_array().unwrap();
        let intercept = coefs[0]["estimate"].as_f64().unwrap();
        let slope = coefs[1]["estimate"].as_f64().unwrap();
        let r2 = env.primary_result["r_squared"].as_f64().unwrap();

        assert!(
            (intercept - (-0.262323073774029)).abs() < 1e-6,
            "intercept mismatch: {}",
            intercept
        );
        assert!(
            (slope - 1.00211681802045).abs() < 1e-6,
            "slope mismatch: {}",
            slope
        );
        assert!(
            (r2 - 0.999993745883712).abs() < 1e-6,
            "R^2 mismatch: {}",
            r2
        );

        // Heteroscedastic synthetic dataset should fail Breusch-Pagan and recommend HC3 robust SEs
        let het_path = dir.path().join("het.parquet");
        let n = 120;
        let xs: Vec<f64> = (1..=n).map(|i| i as f64).collect();
        let ys: Vec<f64> = xs
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                2.0 * x + sign * (x * x) * 0.08
            })
            .collect();
        let het_batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("x", DataType::Float64, false),
                Field::new("y", DataType::Float64, false),
            ])),
            vec![
                Arc::new(Float64Array::from(xs)),
                Arc::new(Float64Array::from(ys)),
            ],
        )
        .unwrap();
        write_parquet(&het_path, &het_batch);

        let het_env = run_stat_regression_fit(
            &StatRegressionFitRequest {
                input_binding: InputBinding {
                    dataset_name: "het_ds".to_string(),
                    parquet_path: het_path.to_string_lossy().to_string(),
                },
                model_type: "ols".to_string(),
                target_col: "y".to_string(),
                feature_cols: vec!["x".to_string()],
                add_intercept: true,
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("het OLS");

        assert!(
            het_env
                .assumption_checks
                .iter()
                .any(|c| c.check == "homoscedasticity_breusch_pagan" && c.status == "failed")
        );
        assert_eq!(
            het_env
                .recommended_alternative
                .as_ref()
                .map(|a| a.method.as_str()),
            Some("ols_hc3_robust_se")
        );
    }

    #[test]
    fn test_stat_correlation_vif_nist_longley_collinearity() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("longley.parquet");

        // NIST StRD Longley macroeconomic dataset (16 observations, famous extreme multicollinearity)
        let x1 = vec![
            83.0, 88.5, 88.2, 89.5, 96.2, 98.1, 99.0, 100.0, 101.2, 104.6, 108.4, 110.8, 112.6,
            114.2, 115.7, 116.9,
        ];
        let x2 = vec![
            234289.0, 259426.0, 258054.0, 284599.0, 328975.0, 346999.0, 365385.0, 363112.0,
            397469.0, 419180.0, 442769.0, 444546.0, 482704.0, 502601.0, 518173.0, 554894.0,
        ];
        let x3 = vec![
            107608.0, 108632.0, 109773.0, 110929.0, 112075.0, 113270.0, 115094.0, 116219.0,
            117388.0, 118734.0, 120445.0, 121950.0, 123366.0, 125368.0, 127852.0, 130081.0,
        ];
        let schema = Arc::new(Schema::new(vec![
            Field::new("gnp_deflator", DataType::Float64, false),
            Field::new("gnp", DataType::Float64, false),
            Field::new("population", DataType::Float64, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Float64Array::from(x1)),
                Arc::new(Float64Array::from(x2)),
                Arc::new(Float64Array::from(x3)),
            ],
        )
        .unwrap();
        write_parquet(&path, &batch);

        let cfg = sci_config();
        let prov = ProvenanceStore::default();
        let cat = FieldStatisticalCatalog::default();

        let env = run_stat_correlation_vif(
            &StatCorrelationVifRequest {
                input_binding: InputBinding {
                    dataset_name: "longley".to_string(),
                    parquet_path: path.to_string_lossy().to_string(),
                },
                columns: vec![
                    "gnp_deflator".to_string(),
                    "gnp".to_string(),
                    "population".to_string(),
                ],
                methods: vec!["pearson".to_string(), "spearman".to_string()],
                control_cols: vec![],
                correction: "fdr_bh".to_string(),
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("longley correlation/VIF");

        let vif_gnp = env.primary_result["vif"]["gnp"].as_f64().unwrap();
        assert!(vif_gnp > 10.0, "expected severe VIF > 10 on Longley GNP, got {}", vif_gnp);
        assert!(
            env.assumption_checks
                .iter()
                .any(|c| c.check == "vif_multicollinearity_gnp" && c.status == "failed")
        );
    }

    #[test]
    fn test_stat_power_analysis_closed_form_matches_scipy() {
        let cfg = sci_config();
        let prov = ProvenanceStore::default();
        let cat = FieldStatisticalCatalog::default();

        // Standard two-sample t-test power: d = 0.5, alpha = 0.05, power = 0.80 -> n ~= 64 per group
        let env = run_stat_power_analysis(
            &StatPowerAnalysisRequest {
                test_type: "two_sample_mean".to_string(),
                solve_for: "sample_size".to_string(),
                alpha: 0.05,
                power: Some(0.80),
                effect_size: Some(0.50),
                p1: None,
                p2: None,
                n_per_group: None,
                ratio: 1.0,
                num_groups: 2,
                alternative: "two-sided".to_string(),
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("power analysis");

        let req_n = env.primary_result["required_n_per_group"].as_u64().unwrap();
        assert_eq!(req_n, 64, "expected n=64 per group for d=0.5, power=0.80, alpha=0.05");
    }

    #[test]
    fn test_stat_hypothesis_test_welch_mwu_fdr_and_simpsons_paradox() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("simpson.parquet");

        // Construct Simpson's paradox table:
        // Enterprise (high base rate): Control 80/100 (80%), Treatment 312/400 (78%) -> diff = -2%
        // SMB (low base rate):         Control 160/400 (40%), Treatment 38/100 (38%) -> diff = -2%
        // Aggregate: Control 240/500 (48%), Treatment 350/500 (70%) -> diff = +22%!
        let mut variants = Vec::new();
        let mut segments = Vec::new();
        let mut convs = Vec::new();

        for i in 0..100 {
            variants.push("control");
            segments.push("enterprise");
            convs.push(if i < 80 { 1.0 } else { 0.0 });
        }
        for i in 0..400 {
            variants.push("treatment");
            segments.push("enterprise");
            convs.push(if i < 312 { 1.0 } else { 0.0 });
        }
        for i in 0..400 {
            variants.push("control");
            segments.push("smb");
            convs.push(if i < 160 { 1.0 } else { 0.0 });
        }
        for i in 0..100 {
            variants.push("treatment");
            segments.push("smb");
            convs.push(if i < 38 { 1.0 } else { 0.0 });
        }

        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("variant", DataType::Utf8, false),
                Field::new("segment", DataType::Utf8, false),
                Field::new("converted", DataType::Float64, false),
            ])),
            vec![
                Arc::new(StringArray::from(variants)),
                Arc::new(StringArray::from(segments)),
                Arc::new(Float64Array::from(convs)),
            ],
        )
        .unwrap();
        write_parquet(&path, &batch);

        let cfg = sci_config();
        let prov = ProvenanceStore::default();
        let cat = FieldStatisticalCatalog::default();

        let env = run_stat_hypothesis_test(
            &StatHypothesisTestRequest {
                input_binding: InputBinding {
                    dataset_name: "ab_exp".to_string(),
                    parquet_path: path.to_string_lossy().to_string(),
                },
                method: "welch_t".to_string(),
                value_col: "converted".to_string(),
                group_col: Some("variant".to_string()),
                col_a: None,
                col_b: None,
                alpha: 0.05,
                alternative: "two-sided".to_string(),
                correction: "none".to_string(),
                segment_col: None,
                stratify_by: Some("segment".to_string()),
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("simpson hypothesis test");

        assert_eq!(
            env.primary_result["stratified_analysis"]["simpsons_paradox_detected"].as_bool(),
            Some(true)
        );
        assert!(
            env.assumption_checks
                .iter()
                .any(|c| c.check == "confounding_simpsons_paradox_segment" && c.status == "failed")
        );
    }

    #[test]
    fn test_stat_timeseries_stl_adf_kpss_and_spurious_random_walks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ts_walks.parquet");

        // 140 days: period-7 seasonal series + two independent positive-drift random walks
        let n = 140usize;
        let mut days = Vec::with_capacity(n);
        let mut seasonal_series = Vec::with_capacity(n);
        let mut walk_a = Vec::with_capacity(n);
        let mut walk_b = Vec::with_capacity(n);

        let mut rng: u64 = 42;
        let mut next_step = || -> f64 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let u = (rng % 1000) as f64 / 1000.0;
            (u - 0.5) * 2.0
        };

        let mut a = 10.0;
        let mut b = 20.0;
        for i in 0..n {
            days.push(format!("2026-01-{:03}", i + 1));
            let s = 100.0 + 0.2 * (i as f64) + 15.0 * (2.0 * std::f64::consts::PI * (i as f64) / 7.0).sin();
            seasonal_series.push(s);
            a += 0.6 + next_step();
            b += 0.5 + next_step();
            walk_a.push(a);
            walk_b.push(b);
        }

        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("day", DataType::Utf8, false),
                Field::new("seasonal_val", DataType::Float64, false),
                Field::new("walk_a", DataType::Float64, false),
                Field::new("walk_b", DataType::Float64, false),
            ])),
            vec![
                Arc::new(StringArray::from(days)),
                Arc::new(Float64Array::from(seasonal_series)),
                Arc::new(Float64Array::from(walk_a)),
                Arc::new(Float64Array::from(walk_b)),
            ],
        )
        .unwrap();
        write_parquet(&path, &batch);

        let cfg = sci_config();
        let prov = ProvenanceStore::default();
        let cat = FieldStatisticalCatalog::default();

        // 1. STL seasonal recovery + ETS forecast
        let stl_env = run_stat_timeseries_analyze(
            &StatTimeseriesAnalyzeRequest {
                input_binding: InputBinding {
                    dataset_name: "ts_ds".to_string(),
                    parquet_path: path.to_string_lossy().to_string(),
                },
                time_col: "day".to_string(),
                value_col: "seasonal_val".to_string(),
                compare_col: None,
                period: Some(7),
                forecast_horizon: Some(7),
                model: "ets".to_string(),
                anomaly_z_threshold: 3.0,
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("STL + ETS");

        let strength = stl_env.primary_result["stl"]["seasonal_strength"]
            .as_f64()
            .unwrap();
        assert!(strength > 0.90, "expected strong period-7 seasonality, got {}", strength);
        assert_eq!(
            stl_env.primary_result["forecast"]["mean"]
                .as_array()
                .unwrap()
                .len(),
            7
        );

        // 2. Spurious correlation between two independent drifting random walks
        let walk_env = run_stat_timeseries_analyze(
            &StatTimeseriesAnalyzeRequest {
                input_binding: InputBinding {
                    dataset_name: "ts_ds".to_string(),
                    parquet_path: path.to_string_lossy().to_string(),
                },
                time_col: "day".to_string(),
                value_col: "walk_a".to_string(),
                compare_col: Some("walk_b".to_string()),
                period: None,
                forecast_horizon: None,
                model: "ets".to_string(),
                anomaly_z_threshold: 3.0,
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("spurious walks");

        let cc = &walk_env.primary_result["cross_series_correlation"];
        assert!(cc["level_pearson_r"].as_f64().unwrap() > 0.85);
        assert!(cc["first_diff_pearson_r"].as_f64().unwrap().abs() < 0.25);
        assert_eq!(cc["spurious_nonstationary_correlation"].as_bool(), Some(true));
        assert_eq!(
            walk_env
                .recommended_alternative
                .as_ref()
                .map(|a| a.method.as_str()),
            Some("first_differenced_correlation")
        );
    }

    #[test]
    fn test_diagnostic_plots_respect_max_plot_points_bound() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("big_plot.parquet");
        let vals: Vec<f64> = (0..500).map(|i| i as f64).collect();
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new("v", DataType::Float64, false)])),
            vec![Arc::new(Float64Array::from(vals))],
        )
        .unwrap();
        write_parquet(&path, &batch);

        let mut cfg = sci_config();
        cfg.max_plot_points = 50;
        let prov = ProvenanceStore::default();
        let cat = FieldStatisticalCatalog::default();

        let env = run_stat_describe(
            &StatDescribeRequest {
                input_binding: InputBinding {
                    dataset_name: "plot_ds".to_string(),
                    parquet_path: path.to_string_lossy().to_string(),
                },
                columns: vec!["v".to_string()],
                sampling: None,
            },
            &cfg,
            &prov,
            &cat,
        )
        .expect("describe with bounded plot points");

        let qq = env
            .diagnostic_plots
            .iter()
            .find(|p| p.plot_type == "qq_plot")
            .expect("qq_plot");
        let pts = qq.series[0]["points"].as_array().unwrap();
        assert!(pts.len() <= 50, "expected <= 50 plot points, got {}", pts.len());
    }
}
