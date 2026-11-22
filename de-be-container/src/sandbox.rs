use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use arrow::array::{Array, Float64Array, Int64Array, RecordBatch, StringArray, UInt32Array};
use arrow::compute::{concat_batches, take_record_batch};
use arrow::datatypes::DataType;
use arrow::ipc::writer::FileWriter as ArrowIpcFileWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::config::SandboxConfig;
use crate::provenance::{ProvenanceError, ProvenanceStore, TrustSummary};

pub const TRAP_05_ZSCORE_CHURN_PYTHON_CODE: &str = r#"
import math
import statistics

def compute(rows):
    totals = [float(r["order_total_usd"]) for r in rows]
    n = len(totals)
    if n == 0:
        return {"anomaly_count": 0, "anomalous_order_ids": [], "mean_usd": 0.0, "std_usd": 0.0}
    mean_val = sum(totals) / float(n)
    variance = sum((x - mean_val) ** 2 for x in totals) / float(n)
    std_val = math.sqrt(variance)

    anomalous_orders = []
    customer_risk = {}
    for r in rows:
        amt = float(r["order_total_usd"])
        z = (amt - mean_val) / std_val if std_val > 0.0 else 0.0
        if z > 3.0:
            anomalous_orders.append({
                "order_id": r["order_id"],
                "cust_ref_key": r["cust_ref_key"],
                "order_total_usd": round(amt, 2),
                "z_score": round(z, 3)
            })
        cust = r["cust_ref_key"]
        status_weight = 1.25 if r.get("status") == "shipped" else 1.0
        customer_risk[cust] = round(customer_risk.get(cust, 0.0) + max(z, 0.0) * status_weight, 4)

    return {
        "anomaly_count": len(anomalous_orders),
        "anomalous_order_ids": [o["order_id"] for o in anomalous_orders],
        "mean_usd": round(mean_val, 2),
        "std_usd": round(std_val, 2),
        "high_risk_customer_count": sum(1 for score in customer_risk.values() if score > 3.0)
    }
"#;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("arrow error reading parquet for sandbox: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("parquet error reading dataset for sandbox: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("json serialization error in sandbox: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Provenance(#[from] ProvenanceError),

    #[error("python sandbox execution timed out after {0} ms")]
    Timeout(u64),

    #[error("python sandbox runtime error: {0}")]
    RuntimeFailure(String),

    #[error("row limit exceeded: {actual_rows} rows exceeds max_sandbox_rows ({max_rows}); {hint}")]
    RowLimitExceeded {
        actual_rows: usize,
        max_rows: usize,
        hint: String,
    },

    #[error("invalid sampling specification: {0}")]
    InvalidSamplingSpec(String),

    #[error("arrow IPC staging error: {0}")]
    ArrowIpcWrite(String),

    #[error("sandbox security violation: {0:?}")]
    SecurityViolation(Vec<String>),

    #[error("built-in algorithm '{0}' not found")]
    BuiltinAlgorithmNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SamplingStrategy {
    Random,
    Stratified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SamplingSpec {
    pub strategy: SamplingStrategy,
    pub max_rows: usize,
    pub seed: u64,
    #[serde(default)]
    pub stratify_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SampleContext {
    pub population_rows: usize,
    pub analyzed_rows: usize,
    pub sampled: bool,
    #[serde(default)]
    pub sampling_strategy: Option<SamplingStrategy>,
    #[serde(default)]
    pub sampling_seed: Option<u64>,
    #[serde(default)]
    pub group_sizes: BTreeMap<String, usize>,
}

/// Ephemeral Arrow IPC staging directory under `sandbox.shm_dir` with guaranteed Drop cleanup.
pub struct ShmRunDir {
    path: PathBuf,
}

impl ShmRunDir {
    pub fn create(shm_dir: &Path) -> Result<Self, SandboxError> {
        fs::create_dir_all(shm_dir)?;
        let run_dir = shm_dir.join(format!("de_run_{}", Uuid::new_v4()));
        fs::create_dir_all(&run_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&run_dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self { path: run_dir })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ShmRunDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AadSkillPromotionManifest {
    pub skill_name: String,
    pub algorithm_version: String,
    pub code_sha256: String,
    pub description: String,
    pub allowed_modules: Vec<String>,
    pub mcp_tool_binding: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PythonAlgorithmValidationRequest {
    pub skill_name: String,
    pub description: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PythonAlgorithmValidationResult {
    pub is_valid: bool,
    pub code_sha256: String,
    pub violations: Vec<String>,
    pub aad_skill_manifest: Option<AadSkillPromotionManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonAlgorithmExecuteRequest {
    pub skill_name: String,
    pub description: String,
    pub dataset_name: String,
    pub parquet_path: String,
    pub code: String,
    pub verified_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonAlgorithmExecuteResult {
    pub executed: bool,
    pub validation: PythonAlgorithmValidationResult,
    pub output: Option<serde_json::Value>,
    pub provenance_run_id: Option<String>,
}

const AST_VALIDATOR_SCRIPT: &str = r#"
import ast
import json
import sys

payload = json.loads(sys.stdin.read())
code = payload["code"]
allowed_modules = set(payload["allowed_modules"])
required_entrypoint = payload.get("entrypoint", "compute")
forbidden_calls = {
    "open", "eval", "exec", "compile", "__import__",
    "getattr", "setattr", "delattr", "globals", "locals",
    "breakpoint", "input"
}
forbidden_attrs = {
    "__subclasses__", "__bases__", "__globals__", "__builtins__",
    "__class__", "__mro__", "__code__", "__closure__"
}

violations = []
try:
    tree = ast.parse(code)
except SyntaxError as exc:
    print(json.dumps({"violations": [f"SyntaxError: {exc.msg} at line {exc.lineno}"]}))
    sys.exit(0)

has_entrypoint = False
for node in ast.walk(tree):
    if isinstance(node, ast.FunctionDef) and node.name == required_entrypoint:
        has_entrypoint = True
    elif isinstance(node, ast.Import):
        for alias in node.names:
            root_mod = alias.name.split(".")[0]
            if root_mod not in allowed_modules:
                violations.append(f"Forbidden import '{alias.name}' (allowed: {sorted(allowed_modules)})")
    elif isinstance(node, ast.ImportFrom):
        mod_name = (node.module or "").split(".")[0]
        if mod_name not in allowed_modules:
            violations.append(f"Forbidden from-import '{node.module}' (allowed: {sorted(allowed_modules)})")
    elif isinstance(node, ast.Call):
        if isinstance(node.func, ast.Name) and node.func.id in forbidden_calls:
            violations.append(f"Forbidden builtin call '{node.func.id}()'")
    elif isinstance(node, ast.Attribute):
        if node.attr in forbidden_attrs or node.attr.startswith("__"):
            violations.append(f"Forbidden dunder attribute access '{node.attr}'")

if not has_entrypoint:
    violations.append(f"Missing required top-level entrypoint 'def {required_entrypoint}(...):'")

print(json.dumps({"violations": violations}))
"#;

const SANDBOX_RUNNER_SCRIPT: &str = r#"
import json
import math
import statistics
import sys

payload = json.loads(sys.stdin.read())
code = payload["code"]
rows = payload["rows"]

safe_builtins = {
    "abs": abs,
    "all": all,
    "any": any,
    "bool": bool,
    "dict": dict,
    "enumerate": enumerate,
    "filter": filter,
    "float": float,
    "int": int,
    "len": len,
    "list": list,
    "map": map,
    "max": max,
    "min": min,
    "pow": pow,
    "range": range,
    "reversed": reversed,
    "round": round,
    "set": set,
    "sorted": sorted,
    "str": str,
    "sum": sum,
    "tuple": tuple,
    "zip": zip,
    "__import__": __import__,
}

sandbox_globals = {
    "__builtins__": safe_builtins,
    "math": math,
    "statistics": statistics,
    "json": json,
}

exec(compile(code, "<sandbox_algorithm>", "exec"), sandbox_globals)
compute_fn = sandbox_globals["compute"]
result = compute_fn(rows)
print(json.dumps(result))
"#;

pub fn validate_python_algorithm(
    req: &PythonAlgorithmValidationRequest,
    config: &SandboxConfig,
) -> Result<PythonAlgorithmValidationResult, SandboxError> {
    let mut hasher = Sha256::new();
    hasher.update(req.code.as_bytes());
    let code_sha256 = format!("{:x}", hasher.finalize());

    let mut violations = Vec::new();
    if req.code.trim().is_empty() {
        violations.push("Python algorithm code must not be empty".to_string());
    }
    if req.code.len() > config.max_code_bytes {
        violations.push(format!(
            "Python algorithm code length ({}) exceeds max_code_bytes ({})",
            req.code.len(),
            config.max_code_bytes
        ));
    }

    if violations.is_empty() {
        let input_json = serde_json::json!({
            "code": req.code,
            "allowed_modules": config.allowed_modules,
        });
        let mut child = Command::new(&config.python_binary)
            .arg("-I")
            .arg("-c")
            .arg(AST_VALIDATOR_SCRIPT)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_json.to_string().as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(SandboxError::RuntimeFailure(stderr));
        }
        let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        if let Some(arr) = parsed.get("violations").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(msg) = item.as_str() {
                    violations.push(msg.to_string());
                }
            }
        }
    }

    let is_valid = violations.is_empty();
    let aad_skill_manifest = if is_valid {
        Some(AadSkillPromotionManifest {
            skill_name: req.skill_name.clone(),
            algorithm_version: "1.0.0".to_string(),
            code_sha256: code_sha256.clone(),
            description: req.description.clone(),
            allowed_modules: config.allowed_modules.clone(),
            mcp_tool_binding: "algorithm_execute".to_string(),
        })
    } else {
        None
    };

    Ok(PythonAlgorithmValidationResult {
        is_valid,
        code_sha256,
        violations,
        aad_skill_manifest,
    })
}

fn read_parquet_rows_as_json(
    path: &Path,
) -> Result<Vec<BTreeMap<String, serde_json::Value>>, SandboxError> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;
    let mut rows = Vec::new();

    for batch_res in reader {
        let batch = batch_res?;
        let schema = batch.schema();
        let num_rows = batch.num_rows();
        let mut batch_rows = vec![BTreeMap::new(); num_rows];

        for (col_idx, field) in schema.fields().iter().enumerate() {
            let col = batch.column(col_idx);
            let name = field.name().clone();
            match field.data_type() {
                DataType::Utf8 => {
                    if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
                        for (r_idx, row_map) in batch_rows.iter_mut().enumerate() {
                            if arr.is_null(r_idx) {
                                row_map.insert(name.clone(), serde_json::Value::Null);
                            } else {
                                row_map.insert(
                                    name.clone(),
                                    serde_json::Value::String(arr.value(r_idx).to_string()),
                                );
                            }
                        }
                    }
                }
                DataType::Float64 => {
                    if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
                        for (r_idx, row_map) in batch_rows.iter_mut().enumerate() {
                            if arr.is_null(r_idx) {
                                row_map.insert(name.clone(), serde_json::Value::Null);
                            } else {
                                row_map.insert(
                                    name.clone(),
                                    serde_json::json!(arr.value(r_idx)),
                                );
                            }
                        }
                    }
                }
                DataType::Int64 => {
                    if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
                        for (r_idx, row_map) in batch_rows.iter_mut().enumerate() {
                            if arr.is_null(r_idx) {
                                row_map.insert(name.clone(), serde_json::Value::Null);
                            } else {
                                row_map.insert(
                                    name.clone(),
                                    serde_json::json!(arr.value(r_idx)),
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        rows.extend(batch_rows);
    }

    Ok(rows)
}

pub fn execute_python_algorithm(
    req: &PythonAlgorithmExecuteRequest,
    config: &SandboxConfig,
    provenance_store: &ProvenanceStore,
) -> Result<PythonAlgorithmExecuteResult, SandboxError> {
    let validation = validate_python_algorithm(
        &PythonAlgorithmValidationRequest {
            skill_name: req.skill_name.clone(),
            description: req.description.clone(),
            code: req.code.clone(),
        },
        config,
    )?;

    if !validation.is_valid {
        return Ok(PythonAlgorithmExecuteResult {
            executed: false,
            validation,
            output: None,
            provenance_run_id: None,
        });
    }

    let rows = read_parquet_rows_as_json(Path::new(&req.parquet_path))?;
    let payload = serde_json::json!({
        "code": req.code,
        "rows": rows,
    });

    let mut child = Command::new(&config.python_binary)
        .arg("-I")
        .arg("-c")
        .arg(SANDBOX_RUNNER_SCRIPT)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload.to_string().as_bytes())?;
    }

    let start = Instant::now();
    let timeout = Duration::from_millis(config.timeout_ms);
    loop {
        if let Some(_status) = child.try_wait()? {
            break;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            return Err(SandboxError::Timeout(config.timeout_ms));
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(SandboxError::RuntimeFailure(stderr));
    }

    let parsed_output: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let anomaly_count = parsed_output
        .get("anomaly_count")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let epistemic_status = if req.verified_by.is_some() {
        "verified"
    } else {
        "empirically_tested"
    };
    let mut verified_by_list = Vec::new();
    if let Some(v) = &req.verified_by {
        verified_by_list.push(v.clone());
    }

    let run = provenance_store.record_run(
        &req.skill_name,
        vec![req.dataset_name.clone()],
        vec![format!(
            "aad_skill:{}:{}:sha256:{}",
            req.skill_name, epistemic_status, validation.code_sha256
        )],
        anomaly_count,
        TrustSummary {
            all_rules_human_verified: req.verified_by.is_some(),
            min_rule_confidence: if req.verified_by.is_some() { 1.0 } else { 0.85 },
            verified_by_stewards: verified_by_list,
            pre_aggregation_applied: false,
            unit_conversion_applied: false,
        },
    )?;

    Ok(PythonAlgorithmExecuteResult {
        executed: true,
        validation,
        output: Some(parsed_output),
        provenance_run_id: Some(run.provenance_run_id),
    })
}

const ARROW_SANDBOX_RUNNER_SCRIPT: &str = r#"
import json
import math
import statistics
import sys
import pyarrow as pa
import pyarrow.ipc as pa_ipc

payload = json.loads(sys.stdin.read())
code = payload["code"]
arrow_files = payload["arrow_files"]
params = payload.get("params", {})

inputs = {}
for name, path in arrow_files.items():
    with pa.memory_map(path, "r") as source:
        reader = pa_ipc.open_file(source)
        inputs[name] = reader.read_all()

safe_builtins = {
    "abs": abs,
    "all": all,
    "any": any,
    "bool": bool,
    "dict": dict,
    "enumerate": enumerate,
    "filter": filter,
    "float": float,
    "int": int,
    "isinstance": isinstance,
    "len": len,
    "list": list,
    "map": map,
    "max": max,
    "min": min,
    "pow": pow,
    "range": range,
    "reversed": reversed,
    "round": round,
    "set": set,
    "sorted": sorted,
    "str": str,
    "sum": sum,
    "tuple": tuple,
    "zip": zip,
    "Exception": Exception,
    "ValueError": ValueError,
    "KeyError": KeyError,
    "TypeError": TypeError,
    "ZeroDivisionError": ZeroDivisionError,
    "__import__": __import__,
}

sandbox_globals = {
    "__builtins__": safe_builtins,
    "math": math,
    "statistics": statistics,
    "json": json,
}

exec(compile(code, "<sandbox_arrow_algorithm>", "exec"), sandbox_globals)
run_fn = sandbox_globals["run"]
result = run_fn(inputs, params)
print(json.dumps(result))
"#;

#[derive(Debug, Clone)]
pub struct ArrowSandboxJob {
    pub algorithm_id: String,
    pub code: String,
    pub input_batches: BTreeMap<String, Vec<RecordBatch>>,
    pub params: serde_json::Value,
    pub sampling: Option<SamplingSpec>,
}

#[derive(Debug, Clone)]
pub struct ArrowSandboxOutput {
    pub code_sha256: String,
    pub output: serde_json::Value,
    pub sample_context: SampleContext,
    pub duration_ms: u64,
}

pub trait SandboxExecutor: Send + Sync {
    fn validate_code(
        &self,
        algorithm_id: &str,
        description: &str,
        code: &str,
        entrypoint: &str,
    ) -> Result<PythonAlgorithmValidationResult, SandboxError>;

    fn execute_arrow(&self, job: ArrowSandboxJob) -> Result<ArrowSandboxOutput, SandboxError>;
}

#[derive(Debug, Clone)]
pub struct ProcessSandbox {
    config: SandboxConfig,
}

impl ProcessSandbox {
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }
}

pub fn validate_python_code_with_entrypoint(
    skill_name: &str,
    description: &str,
    code: &str,
    entrypoint: &str,
    config: &SandboxConfig,
) -> Result<PythonAlgorithmValidationResult, SandboxError> {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    let code_sha256 = format!("{:x}", hasher.finalize());

    let mut violations = Vec::new();
    if code.trim().is_empty() {
        violations.push("Python algorithm code must not be empty".to_string());
    }
    if code.len() > config.max_code_bytes {
        violations.push(format!(
            "Python algorithm code length ({}) exceeds max_code_bytes ({})",
            code.len(),
            config.max_code_bytes
        ));
    }

    if violations.is_empty() {
        let input_json = serde_json::json!({
            "code": code,
            "allowed_modules": config.allowed_modules,
            "entrypoint": entrypoint,
        });
        let mut child = Command::new(&config.python_binary)
            .arg("-I")
            .arg("-c")
            .arg(AST_VALIDATOR_SCRIPT)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_json.to_string().as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(SandboxError::RuntimeFailure(stderr));
        }
        let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        if let Some(arr) = parsed.get("violations").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(msg) = item.as_str() {
                    violations.push(msg.to_string());
                }
            }
        }
    }

    let is_valid = violations.is_empty();
    let aad_skill_manifest = if is_valid {
        Some(AadSkillPromotionManifest {
            skill_name: skill_name.to_string(),
            algorithm_version: "1.0.0".to_string(),
            code_sha256: code_sha256.clone(),
            description: description.to_string(),
            allowed_modules: config.allowed_modules.clone(),
            mcp_tool_binding: "algorithm_execute".to_string(),
        })
    } else {
        None
    };

    Ok(PythonAlgorithmValidationResult {
        is_valid,
        code_sha256,
        violations,
        aad_skill_manifest,
    })
}

fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    if x == 0 {
        x = 0x9E37_79B9_7F4A_7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

fn deterministic_shuffle_indices(indices: &mut [u32], seed: u64) {
    let mut rng = seed ^ 0xA5A5_5A5A_1234_5678;
    if indices.len() <= 1 {
        return;
    }
    for i in (1..indices.len()).rev() {
        let j = (xorshift64(&mut rng) as usize) % (i + 1);
        indices.swap(i, j);
    }
}

fn extract_stratum_key(batch: &RecordBatch, col_name: &str, row: usize) -> Result<String, SandboxError> {
    let col_idx = batch.schema().index_of(col_name).map_err(|_| {
        SandboxError::InvalidSamplingSpec(format!("stratify_by column '{}' not found", col_name))
    })?;
    let col = batch.column(col_idx);
    if col.is_null(row) {
        return Ok("__NULL__".to_string());
    }
    match col.data_type() {
        DataType::Utf8 => Ok(col
            .as_any()
            .downcast_ref::<StringArray>()
            .map(|a| a.value(row).to_string())
            .unwrap_or_default()),
        DataType::Int64 => Ok(col
            .as_any()
            .downcast_ref::<Int64Array>()
            .map(|a| a.value(row).to_string())
            .unwrap_or_default()),
        DataType::Float64 => Ok(col
            .as_any()
            .downcast_ref::<Float64Array>()
            .map(|a| format!("{:.4}", a.value(row)))
            .unwrap_or_default()),
        other => Err(SandboxError::InvalidSamplingSpec(format!(
            "unsupported stratify_by column type {:?}",
            other
        ))),
    }
}

/// Reads all Arrow RecordBatches from a Parquet file.
pub fn read_parquet_batches(path: &Path) -> Result<Vec<RecordBatch>, SandboxError> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let reader = builder.build()?;
    let mut batches = Vec::new();
    for batch_res in reader {
        batches.push(batch_res?);
    }
    Ok(batches)
}

/// Stages Arrow RecordBatches to an Arrow IPC File inside `run_dir`, enforcing
/// `config.max_sandbox_rows` fail-fast unless explicit `SamplingSpec` is supplied.
pub fn stage_batches_to_arrow_ipc(
    batches: &[RecordBatch],
    binding_name: &str,
    run_dir: &ShmRunDir,
    config: &SandboxConfig,
    sampling: Option<&SamplingSpec>,
) -> Result<(PathBuf, SampleContext), SandboxError> {
    let population_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    if batches.is_empty() || population_rows == 0 {
        return Err(SandboxError::ArrowIpcWrite(format!(
            "input binding '{}' has zero rows",
            binding_name
        )));
    }

    let schema = batches[0].schema();
    let combined = concat_batches(&schema, batches)?;

    let (staged_batch, sample_ctx) = if let Some(spec) = sampling {
        if spec.max_rows == 0 {
            return Err(SandboxError::InvalidSamplingSpec(
                "sampling.max_rows must be > 0".to_string(),
            ));
        }
        let target_n = spec.max_rows.min(config.max_sandbox_rows).min(population_rows);
        if target_n == population_rows {
            (
                combined,
                SampleContext {
                    population_rows,
                    analyzed_rows: population_rows,
                    sampled: false,
                    sampling_strategy: None,
                    sampling_seed: None,
                    group_sizes: BTreeMap::new(),
                },
            )
        } else {
            let mut chosen: Vec<u32> = match spec.strategy {
                SamplingStrategy::Random => {
                    let mut all: Vec<u32> = (0..population_rows as u32).collect();
                    deterministic_shuffle_indices(&mut all, spec.seed);
                    all.truncate(target_n);
                    all.sort_unstable();
                    all
                }
                SamplingStrategy::Stratified => {
                    let col_name = spec.stratify_by.as_deref().ok_or_else(|| {
                        SandboxError::InvalidSamplingSpec(
                            "stratified sampling requires stratify_by column".to_string(),
                        )
                    })?;
                    let mut strata: BTreeMap<String, Vec<u32>> = BTreeMap::new();
                    for row in 0..population_rows {
                        let key = extract_stratum_key(&combined, col_name, row)?;
                        strata.entry(key).or_default().push(row as u32);
                    }
                    let mut selected = Vec::with_capacity(target_n);
                    let num_strata = strata.len();
                    for (idx, (_, mut rows)) in strata.into_iter().enumerate() {
                        let stratum_seed = spec.seed.wrapping_add((idx as u64 + 1) * 0x9E37_79B9);
                        deterministic_shuffle_indices(&mut rows, stratum_seed);
                        let prop = ((rows.len() as f64 / population_rows as f64) * target_n as f64)
                            .round() as usize;
                        let take_k = prop.max(if target_n >= num_strata { 1 } else { 0 }).min(rows.len());
                        selected.extend(rows.into_iter().take(take_k));
                    }
                    if selected.len() > target_n {
                        selected.truncate(target_n);
                    }
                    selected.sort_unstable();
                    selected
                }
            };
            if chosen.is_empty() {
                chosen.push(0);
            }
            let idx_array = UInt32Array::from(chosen);
            let sampled_batch = take_record_batch(&combined, &idx_array)?;
            let analyzed_rows = sampled_batch.num_rows();
            (
                sampled_batch,
                SampleContext {
                    population_rows,
                    analyzed_rows,
                    sampled: true,
                    sampling_strategy: Some(spec.strategy.clone()),
                    sampling_seed: Some(spec.seed),
                    group_sizes: BTreeMap::new(),
                },
            )
        }
    } else if population_rows > config.max_sandbox_rows {
        return Err(SandboxError::RowLimitExceeded {
            actual_rows: population_rows,
            max_rows: config.max_sandbox_rows,
            hint: "Pre-aggregate in DataFusion SQL or pass an explicit deterministic SamplingSpec."
                .to_string(),
        });
    } else {
        (
            combined,
            SampleContext {
                population_rows,
                analyzed_rows: population_rows,
                sampled: false,
                sampling_strategy: None,
                sampling_seed: None,
                group_sizes: BTreeMap::new(),
            },
        )
    };

    let arrow_path = run_dir.path().join(format!("in_{}.arrow", binding_name));
    let out_file = File::create(&arrow_path)?;
    let mut writer = ArrowIpcFileWriter::try_new(out_file, &staged_batch.schema())
        .map_err(|e| SandboxError::ArrowIpcWrite(e.to_string()))?;
    writer
        .write(&staged_batch)
        .map_err(|e| SandboxError::ArrowIpcWrite(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| SandboxError::ArrowIpcWrite(e.to_string()))?;

    Ok((arrow_path, sample_ctx))
}

impl SandboxExecutor for ProcessSandbox {
    fn validate_code(
        &self,
        algorithm_id: &str,
        description: &str,
        code: &str,
        entrypoint: &str,
    ) -> Result<PythonAlgorithmValidationResult, SandboxError> {
        validate_python_code_with_entrypoint(
            algorithm_id,
            description,
            code,
            entrypoint,
            &self.config,
        )
    }

    fn execute_arrow(&self, job: ArrowSandboxJob) -> Result<ArrowSandboxOutput, SandboxError> {
        let validation = self.validate_code(&job.algorithm_id, "", &job.code, "run")?;
        if !validation.is_valid {
            return Err(SandboxError::SecurityViolation(validation.violations));
        }

        let run_dir = ShmRunDir::create(&self.config.shm_dir)?;
        let mut arrow_files = BTreeMap::new();
        let mut aggregate_sample_ctx = SampleContext {
            population_rows: 0,
            analyzed_rows: 0,
            sampled: false,
            sampling_strategy: None,
            sampling_seed: None,
            group_sizes: BTreeMap::new(),
        };

        for (binding_name, batches) in &job.input_batches {
            let (arrow_path, ctx) = stage_batches_to_arrow_ipc(
                batches,
                binding_name,
                &run_dir,
                &self.config,
                job.sampling.as_ref(),
            )?;
            arrow_files.insert(
                binding_name.clone(),
                arrow_path.to_string_lossy().to_string(),
            );
            aggregate_sample_ctx.population_rows += ctx.population_rows;
            aggregate_sample_ctx.analyzed_rows += ctx.analyzed_rows;
            if ctx.sampled {
                aggregate_sample_ctx.sampled = true;
                aggregate_sample_ctx.sampling_strategy = ctx.sampling_strategy;
                aggregate_sample_ctx.sampling_seed = ctx.sampling_seed;
            }
        }

        let payload = serde_json::json!({
            "code": job.code,
            "arrow_files": arrow_files,
            "params": job.params,
        });

        let start = Instant::now();
        let mut child = Command::new(&self.config.python_binary)
            .arg("-I")
            .arg("-c")
            .arg(ARROW_SANDBOX_RUNNER_SCRIPT)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(payload.to_string().as_bytes())?;
        }

        let timeout = Duration::from_millis(self.config.timeout_ms);
        loop {
            if let Some(status) = child.try_wait()? {
                let output = child.wait_with_output()?;
                if !status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    return Err(SandboxError::RuntimeFailure(stderr));
                }
                let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
                let duration_ms = start.elapsed().as_millis() as u64;
                drop(run_dir);
                return Ok(ArrowSandboxOutput {
                    code_sha256: validation.code_sha256,
                    output: parsed,
                    sample_context: aggregate_sample_ctx,
                    duration_ms,
                });
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                drop(run_dir);
                return Err(SandboxError::Timeout(self.config.timeout_ms));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
pub(crate) fn detect_test_python_binary() -> PathBuf {
    let candidates = [
        "../.venv-sandbox/bin/python3",
        ".venv-sandbox/bin/python3",
        "/opt/de-venv/bin/python3",
    ];
    for c in candidates {
        if Path::new(c).exists() {
            return PathBuf::from(c);
        }
    }
    PathBuf::from("python3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SandboxBackend;
    use crate::seeder::seed_validation_suite;
    use arrow::datatypes::{Field, Schema};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn test_sandbox_config() -> SandboxConfig {
        SandboxConfig {
            backend: SandboxBackend::Process,
            python_binary: detect_test_python_binary(),
            timeout_ms: 15_000,
            max_code_bytes: 65_536,
            max_sandbox_rows: 100_000,
            max_plot_points: 1000,
            shm_dir: PathBuf::from("/dev/shm"),
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

    fn make_numeric_batch(n: usize) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("group", DataType::Utf8, false),
            Field::new("val", DataType::Float64, false),
        ]));
        let groups: Vec<&str> = (0..n)
            .map(|i| if i % 3 == 0 { "A" } else if i % 3 == 1 { "B" } else { "C" })
            .collect();
        let vals: Vec<f64> = (0..n).map(|i| i as f64 * 1.5).collect();
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(groups)),
                Arc::new(Float64Array::from(vals)),
            ],
        )
        .expect("record batch")
    }

    #[test]
    fn test_validate_python_blocks_forbidden_imports_and_builtins() {
        let cfg = test_sandbox_config();

        let bad_code = r#"
import os
import subprocess

def compute(rows):
    os.system("echo pwned")
    return eval("1 + 1")
"#;
        let res = validate_python_algorithm(
            &PythonAlgorithmValidationRequest {
                skill_name: "malicious_skill".to_string(),
                description: "Attempts forbidden imports and eval".to_string(),
                code: bad_code.to_string(),
            },
            &cfg,
        )
        .expect("validate returns result");

        assert!(!res.is_valid);
        assert!(res.aad_skill_manifest.is_none());
        assert!(res.violations.iter().any(|v| v.contains("os")));
        assert!(res.violations.iter().any(|v| v.contains("subprocess")));
        assert!(res.violations.iter().any(|v| v.contains("eval")));
    }

    #[test]
    fn test_ast_gatekeeper_allows_scientific_modules_and_blocks_escapes() {
        let cfg = test_sandbox_config();
        let exec = ProcessSandbox::new(cfg);

        let good_code = r#"
import numpy as np
from scipy import stats
import statsmodels.api as sm
import pyarrow as pa

def run(inputs, params):
    return {"ok": True}
"#;
        let valid = exec
            .validate_code("sci_ok", "scientific modules test", good_code, "run")
            .expect("validate");
        assert!(valid.is_valid, "violations: {:?}", valid.violations);

        let escape_code = r#"
import numpy as np

def run(inputs, params):
    x = ().__class__.__bases__
    return {"x": str(x)}
"#;
        let blocked = exec
            .validate_code("sci_bad", "dunder escape", escape_code, "run")
            .expect("validate");
        assert!(!blocked.is_valid);
        assert!(blocked.violations.iter().any(|v| v.contains("__class__")));
    }

    #[test]
    fn test_arrow_ipc_staging_and_drop_guard_cleanup() {
        let shm_root = tempdir().expect("temp shm");
        let mut cfg = test_sandbox_config();
        cfg.shm_dir = shm_root.path().to_path_buf();

        // Direct Drop guard check
        let tracked_path;
        {
            let run_dir = ShmRunDir::create(shm_root.path()).expect("create run dir");
            tracked_path = run_dir.path().to_path_buf();
            assert!(tracked_path.exists());
        }
        assert!(
            !tracked_path.exists(),
            "ShmRunDir must remove directory on drop"
        );

        // End-to-end Arrow IPC execution using pyarrow + numpy
        let exec = ProcessSandbox::new(cfg);
        let mut input_batches = BTreeMap::new();
        input_batches.insert("data".to_string(), vec![make_numeric_batch(10)]);

        let code = r#"
import numpy as np

def run(inputs, params):
    tbl = inputs["data"]
    arr = tbl.column("val").to_numpy(zero_copy_only=False)
    return {"count": int(len(arr)), "mean": float(np.mean(arr))}
"#;
        let out = exec
            .execute_arrow(ArrowSandboxJob {
                algorithm_id: "test_arrow_mean".to_string(),
                code: code.to_string(),
                input_batches,
                params: serde_json::json!({}),
                sampling: None,
            })
            .expect("arrow execution");

        assert_eq!(out.output["count"].as_u64(), Some(10));
        assert!((out.output["mean"].as_f64().unwrap() - 6.75).abs() < 1e-9);
        assert!(!out.sample_context.sampled);
        assert_eq!(out.sample_context.analyzed_rows, 10);

        // Verify zero leaked de_run_* dirs remain under shm_root
        let remaining: Vec<_> = fs::read_dir(shm_root.path()).unwrap().collect();
        assert!(remaining.is_empty(), "expected shm_root to be empty after run");
    }

    #[test]
    fn test_row_limit_fails_fast_without_explicit_sampling() {
        let shm_root = tempdir().expect("temp shm");
        let mut cfg = test_sandbox_config();
        cfg.shm_dir = shm_root.path().to_path_buf();
        cfg.max_sandbox_rows = 50;

        let exec = ProcessSandbox::new(cfg);
        let mut input_batches = BTreeMap::new();
        input_batches.insert("data".to_string(), vec![make_numeric_batch(51)]);

        let err = exec
            .execute_arrow(ArrowSandboxJob {
                algorithm_id: "over_limit".to_string(),
                code: "def run(inputs, params):\n    return {}\n".to_string(),
                input_batches,
                params: serde_json::json!({}),
                sampling: None,
            })
            .expect_err("must fail fast when rows > max_sandbox_rows");

        assert!(matches!(
            err,
            SandboxError::RowLimitExceeded {
                actual_rows: 51,
                max_rows: 50,
                ..
            }
        ));
    }

    #[test]
    fn test_explicit_seeded_sampling_is_deterministic_and_recorded_in_provenance() {
        let shm_root = tempdir().expect("temp shm");
        let mut cfg = test_sandbox_config();
        cfg.shm_dir = shm_root.path().to_path_buf();
        cfg.max_sandbox_rows = 50;

        let exec = ProcessSandbox::new(cfg);
        let batch = make_numeric_batch(200);
        let code = r#"
import numpy as np

def run(inputs, params):
    arr = inputs["data"].column("val").to_numpy(zero_copy_only=False)
    return {"sum": float(np.sum(arr)), "n": int(len(arr))}
"#;
        let spec = SamplingSpec {
            strategy: SamplingStrategy::Random,
            max_rows: 40,
            seed: 42,
            stratify_by: None,
        };

        let mut b1 = BTreeMap::new();
        b1.insert("data".to_string(), vec![batch.clone()]);
        let r1 = exec
            .execute_arrow(ArrowSandboxJob {
                algorithm_id: "det_sample".to_string(),
                code: code.to_string(),
                input_batches: b1,
                params: serde_json::json!({}),
                sampling: Some(spec.clone()),
            })
            .expect("run 1");

        let mut b2 = BTreeMap::new();
        b2.insert("data".to_string(), vec![batch]);
        let r2 = exec
            .execute_arrow(ArrowSandboxJob {
                algorithm_id: "det_sample".to_string(),
                code: code.to_string(),
                input_batches: b2,
                params: serde_json::json!({}),
                sampling: Some(spec),
            })
            .expect("run 2");

        assert_eq!(r1.output, r2.output);
        assert!(r1.sample_context.sampled);
        assert_eq!(r1.sample_context.population_rows, 200);
        assert_eq!(r1.sample_context.analyzed_rows, 40);
        assert_eq!(r1.sample_context.sampling_seed, Some(42));
    }

    #[test]
    fn test_stratified_sampling_preserves_group_representation() {
        let shm_root = tempdir().expect("temp shm");
        let mut cfg = test_sandbox_config();
        cfg.shm_dir = shm_root.path().to_path_buf();
        cfg.max_sandbox_rows = 50;

        let exec = ProcessSandbox::new(cfg);
        let mut input_batches = BTreeMap::new();
        input_batches.insert("data".to_string(), vec![make_numeric_batch(150)]);

        let code = r#"
def run(inputs, params):
    groups = inputs["data"].column("group").to_pylist()
    counts = {}
    for g in groups:
        counts[g] = counts.get(g, 0) + 1
    return {"counts": counts}
"#;
        let out = exec
            .execute_arrow(ArrowSandboxJob {
                algorithm_id: "strat_sample".to_string(),
                code: code.to_string(),
                input_batches,
                params: serde_json::json!({}),
                sampling: Some(SamplingSpec {
                    strategy: SamplingStrategy::Stratified,
                    max_rows: 30,
                    seed: 7,
                    stratify_by: Some("group".to_string()),
                }),
            })
            .expect("stratified run");

        let counts = out.output["counts"].as_object().expect("counts map");
        assert_eq!(counts.len(), 3, "all 3 strata A, B, C must be represented");
        assert!(counts["A"].as_u64().unwrap() >= 8);
        assert!(counts["B"].as_u64().unwrap() >= 8);
        assert!(counts["C"].as_u64().unwrap() >= 8);
    }

    #[test]
    fn test_execute_python_trap_05_anomaly_and_churn_score() {
        let dir = tempdir().expect("tempdir");
        let manifest = seed_validation_suite(42, 100, dir.path()).expect("seed suite");
        let cfg = test_sandbox_config();
        let store = ProvenanceStore::default();

        let orders_path = dir.path().join("sales_orders.parquet");
        let exec_res = execute_python_algorithm(
            &PythonAlgorithmExecuteRequest {
                skill_name: "trap_05_zscore_anomaly_detector".to_string(),
                description: "Detects z_score > 3.0 order anomalies and composite churn risk"
                    .to_string(),
                dataset_name: "sales_orders".to_string(),
                parquet_path: orders_path.to_string_lossy().to_string(),
                code: TRAP_05_ZSCORE_CHURN_PYTHON_CODE.to_string(),
                verified_by: Some("steward@polecatworks.io".to_string()),
            },
            &cfg,
            &store,
        )
        .expect("execute trap 05 algorithm");

        assert!(exec_res.executed);
        assert!(exec_res.validation.is_valid);
        let skill_manifest = exec_res
            .validation
            .aad_skill_manifest
            .expect("aad skill manifest");
        assert_eq!(skill_manifest.mcp_tool_binding, "algorithm_execute");

        let out = exec_res.output.expect("sandbox json output");
        let anomaly_count = out["anomaly_count"].as_u64().expect("anomaly_count") as usize;
        assert_eq!(anomaly_count, manifest.expected_trap_05_anomaly_count);
        assert!(anomaly_count >= 3);

        let run_id = exec_res.provenance_run_id.expect("provenance run id");
        let run = store.get_run(&run_id).expect("fetch provenance run");
        assert!(
            run.rules_applied
                .iter()
                .any(|r| r.contains(&exec_res.validation.code_sha256))
        );
    }
}
