use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use arrow::array::{Array, Float64Array, Int64Array, StringArray};
use arrow::datatypes::DataType;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

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

has_compute_fn = False
for node in ast.walk(tree):
    if isinstance(node, ast.FunctionDef) and node.name == "compute":
        has_compute_fn = True
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

if not has_compute_fn:
    violations.append("Missing required top-level entrypoint 'def compute(rows):'")

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeder::seed_validation_suite;
    use tempfile::tempdir;

    fn test_sandbox_config() -> SandboxConfig {
        SandboxConfig {
            python_binary: "python3".to_string(),
            timeout_ms: 5_000,
            max_code_bytes: 16_384,
            allowed_modules: vec![
                "math".to_string(),
                "statistics".to_string(),
                "json".to_string(),
            ],
        }
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
        assert!(
            res.violations.iter().any(|v| v.contains("os")),
            "expected violation for os import: {:?}",
            res.violations
        );
        assert!(
            res.violations.iter().any(|v| v.contains("subprocess")),
            "expected violation for subprocess import: {:?}",
            res.violations
        );
        assert!(
            res.violations.iter().any(|v| v.contains("eval")),
            "expected violation for eval() call: {:?}",
            res.violations
        );
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
