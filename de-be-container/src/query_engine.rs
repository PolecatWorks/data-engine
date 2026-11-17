use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use arrow::array::{Array, Float64Array, Int64Array, StringArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::provenance::{ProvenanceError, ProvenanceRun, ProvenanceStore, TrustSummary};

#[derive(Debug, Error)]
pub enum QueryEngineError {
    #[error("I/O error in query engine: {0}")]
    Io(#[from] std::io::Error),

    #[error("parquet error in query engine: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("arrow error in query engine: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("provenance error: {0}")]
    Provenance(#[from] ProvenanceError),

    #[error("column '{column}' not found in dataset '{dataset}'")]
    ColumnNotFound { dataset: String, column: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticRuleBinding {
    pub rule_id: String,
    /// One of: "JOINABLE_WITH", "FORBIDDEN_JOIN", "UNIT_CONVERSION", "DRIFT_ALERT"
    pub rule_type: String,
    pub left_dataset: String,
    pub left_field: String,
    #[serde(default)]
    pub right_dataset: Option<String>,
    #[serde(default)]
    pub right_field: Option<String>,
    #[serde(default = "default_cardinality")]
    pub cardinality: String,
    #[serde(default = "default_one")]
    pub confidence: f64,
    #[serde(default = "default_verified")]
    pub epistemic_status: String,
    #[serde(default)]
    pub verified_by: Option<String>,
    #[serde(default = "default_one")]
    pub scale_divisor: f64,
}

fn default_cardinality() -> String {
    "N:1".to_string()
}

fn default_one() -> f64 {
    1.0
}

fn default_verified() -> String {
    "verified".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticQueryPlan {
    pub query_name: String,
    pub left_dataset: String,
    pub left_parquet: String,
    pub left_join_field: Option<String>,
    pub right_dataset: Option<String>,
    pub right_parquet: Option<String>,
    pub right_join_field: Option<String>,
    pub metric_dataset: String,
    pub metric_field: String,
    /// Either "SUM", "MEAN", or "COUNT_JOINED"
    pub aggregation: String,
    #[serde(default)]
    pub semantic_rules: Vec<SemanticRuleBinding>,
    /// If true, halts execution when any rule is pending_review or drifted
    #[serde(default = "default_true")]
    pub enforce_epistemic_gate: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryValidationResult {
    pub status: String,
    pub is_valid_to_execute: bool,
    pub requires_steward_review: bool,
    pub blocked_forbidden_joins: Vec<String>,
    pub fan_out_warnings: Vec<String>,
    pub drift_warnings: Vec<String>,
    pub pre_aggregation_rewrite_required: bool,
    pub unit_scale_divisor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryExecutionResult {
    pub validation: QueryValidationResult,
    pub executed: bool,
    pub computed_value: Option<f64>,
    pub rows_evaluated: usize,
    pub provenance: Option<ProvenanceRun>,
}

pub fn validate_query_plan(plan: &SemanticQueryPlan) -> QueryValidationResult {
    let mut blocked_forbidden_joins = Vec::new();
    let mut fan_out_warnings = Vec::new();
    let mut drift_warnings = Vec::new();
    let mut pre_aggregation_rewrite_required = false;
    let mut unit_scale_divisor = 1.0_f64;
    let mut has_unverified_rule = false;

    for rule in &plan.semantic_rules {
        if rule.epistemic_status == "pending_review" || rule.epistemic_status == "hypothesized" {
            has_unverified_rule = true;
        }
        if rule.epistemic_status == "drifted" || rule.rule_type == "DRIFT_ALERT" {
            drift_warnings.push(format!(
                "Dataset/field '{}.{}' is marked drifted (rule '{}'); human steward unit verification required.",
                rule.left_dataset, rule.left_field, rule.rule_id
            ));
        }
        if rule.rule_type == "FORBIDDEN_JOIN" {
            let right_ds = rule.right_dataset.as_deref().unwrap_or("");
            let right_fl = rule.right_field.as_deref().unwrap_or("");
            blocked_forbidden_joins.push(format!(
                "FORBIDDEN_JOIN blocked between '{}.{}' and '{}.{}' (rule '{}', verified_by: {:?})",
                rule.left_dataset,
                rule.left_field,
                right_ds,
                right_fl,
                rule.rule_id,
                rule.verified_by
            ));
        }
        if rule.rule_type == "JOINABLE_WITH" && rule.cardinality == "N:M" {
            fan_out_warnings.push(format!(
                "N:M join detected between '{}' and '{}' (rule '{}'); pre-aggregating metric '{}' by join key before joining.",
                rule.left_dataset,
                rule.right_dataset.as_deref().unwrap_or(""),
                rule.rule_id,
                plan.metric_field
            ));
            pre_aggregation_rewrite_required = true;
        }
        if rule.rule_type == "UNIT_CONVERSION" && rule.epistemic_status == "verified" {
            unit_scale_divisor = rule.scale_divisor;
        }
    }

    let requires_steward_review =
        !drift_warnings.is_empty() || (plan.enforce_epistemic_gate && has_unverified_rule);

    let (status, is_valid_to_execute) = if !blocked_forbidden_joins.is_empty() {
        ("blocked_forbidden_join".to_string(), false)
    } else if requires_steward_review {
        ("requires_steward_review".to_string(), false)
    } else {
        ("valid".to_string(), true)
    };

    QueryValidationResult {
        status,
        is_valid_to_execute,
        requires_steward_review,
        blocked_forbidden_joins,
        fan_out_warnings,
        drift_warnings,
        pre_aggregation_rewrite_required,
        unit_scale_divisor,
    }
}

fn read_key_and_metric_columns(
    dataset_name: &str,
    parquet_path: &Path,
    key_col: Option<&str>,
    metric_col: Option<&str>,
) -> Result<Vec<(String, f64)>, QueryEngineError> {
    let file = File::open(parquet_path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();

    let key_idx = if let Some(kc) = key_col {
        Some(
            schema
                .fields()
                .iter()
                .position(|f| f.name() == kc)
                .ok_or_else(|| QueryEngineError::ColumnNotFound {
                    dataset: dataset_name.to_string(),
                    column: kc.to_string(),
                })?,
        )
    } else {
        None
    };

    let metric_idx = if let Some(mc) = metric_col {
        Some(
            schema
                .fields()
                .iter()
                .position(|f| f.name() == mc)
                .ok_or_else(|| QueryEngineError::ColumnNotFound {
                    dataset: dataset_name.to_string(),
                    column: mc.to_string(),
                })?,
        )
    } else {
        None
    };

    let reader = builder.build()?;
    let mut rows = Vec::new();

    for batch_res in reader {
        let batch = batch_res?;
        for row_i in 0..batch.num_rows() {
            let key_str = if let Some(k_i) = key_idx {
                let col = batch.column(k_i);
                if let Some(s_arr) = col.as_any().downcast_ref::<StringArray>() {
                    s_arr.value(row_i).to_string()
                } else if let Some(i_arr) = col.as_any().downcast_ref::<Int64Array>() {
                    i_arr.value(row_i).to_string()
                } else {
                    String::new()
                }
            } else {
                format!("row_{}", row_i)
            };

            let metric_val = if let Some(m_i) = metric_idx {
                let col = batch.column(m_i);
                if let Some(f_arr) = col.as_any().downcast_ref::<Float64Array>() {
                    if f_arr.is_null(row_i) {
                        0.0
                    } else {
                        f_arr.value(row_i)
                    }
                } else if let Some(i_arr) = col.as_any().downcast_ref::<Int64Array>() {
                    if i_arr.is_null(row_i) {
                        0.0
                    } else {
                        i_arr.value(row_i) as f64
                    }
                } else {
                    1.0
                }
            } else {
                1.0
            };

            rows.push((key_str, metric_val));
        }
    }

    Ok(rows)
}

pub fn execute_query_plan(
    plan: &SemanticQueryPlan,
    provenance_store: &ProvenanceStore,
) -> Result<QueryExecutionResult, QueryEngineError> {
    let validation = validate_query_plan(plan);
    if !validation.is_valid_to_execute {
        return Ok(QueryExecutionResult {
            validation,
            executed: false,
            computed_value: None,
            rows_evaluated: 0,
            provenance: None,
        });
    }

    let left_path = Path::new(&plan.left_parquet);
    let left_metric = if plan.metric_dataset == plan.left_dataset {
        Some(plan.metric_field.as_str())
    } else {
        None
    };

    let (raw_computed, rows_evaluated) = if let (
        Some(right_ds),
        Some(right_parquet),
        Some(left_jf),
        Some(right_jf),
    ) = (
        plan.right_dataset.as_deref(),
        plan.right_parquet.as_deref(),
        plan.left_join_field.as_deref(),
        plan.right_join_field.as_deref(),
    ) {
        let right_path = Path::new(right_parquet);
        let right_metric = if plan.metric_dataset == right_ds {
            Some(plan.metric_field.as_str())
        } else {
            None
        };

        let left_rows =
            read_key_and_metric_columns(&plan.left_dataset, left_path, Some(left_jf), left_metric)?;
        let right_rows =
            read_key_and_metric_columns(right_ds, right_path, Some(right_jf), right_metric)?;

        if validation.pre_aggregation_rewrite_required {
            // Safe Pre-Aggregation CTE Rewrite: aggregate the metric table by join key FIRST,
            // and join against the distinct key set of the partner table so 1:N * 1:N fan-out never multiplies values!
            let mut left_keys: HashMap<String, f64> = HashMap::new();
            for (k, v) in &left_rows {
                *left_keys.entry(k.clone()).or_insert(0.0) += *v;
            }
            let mut right_keys: HashMap<String, f64> = HashMap::new();
            for (k, v) in &right_rows {
                *right_keys.entry(k.clone()).or_insert(0.0) += *v;
            }

            let mut total = 0.0_f64;
            let mut matched_keys = 0_usize;
            for (k, l_sum) in &left_keys {
                if let Some(r_sum) = right_keys.get(k) {
                    matched_keys += 1;
                    if plan.metric_dataset == plan.left_dataset {
                        total += *l_sum;
                    } else {
                        total += *r_sum;
                    }
                }
            }
            (total, matched_keys)
        } else {
            // Standard relational inner join
            let mut right_multimap: HashMap<String, Vec<f64>> = HashMap::new();
            for (k, v) in right_rows {
                right_multimap.entry(k).or_default().push(v);
            }

            let mut total = 0.0_f64;
            let mut joined_rows = 0_usize;
            for (l_key, l_val) in left_rows {
                if let Some(r_vals) = right_multimap.get(&l_key) {
                    for r_val in r_vals {
                        joined_rows += 1;
                        if plan.aggregation == "COUNT_JOINED" {
                            total += 1.0;
                        } else if plan.metric_dataset == plan.left_dataset {
                            total += l_val;
                        } else {
                            total += *r_val;
                        }
                    }
                }
            }
            let val = if plan.aggregation == "MEAN" && joined_rows > 0 {
                total / (joined_rows as f64)
            } else {
                total
            };
            (val, joined_rows)
        }
    } else {
        // Single-table aggregation
        let rows = read_key_and_metric_columns(
            &plan.left_dataset,
            left_path,
            None,
            Some(plan.metric_field.as_str()),
        )?;
        let count = rows.len();
        let sum: f64 = rows.iter().map(|(_, v)| *v).sum();
        let val = if plan.aggregation == "MEAN" && count > 0 {
            sum / (count as f64)
        } else {
            sum
        };
        (val, count)
    };

    let scaled_value =
        ((raw_computed / validation.unit_scale_divisor) * 100.0).round() / 100.0;

    let mut datasets_consulted = vec![plan.left_dataset.clone()];
    if let Some(rds) = &plan.right_dataset {
        datasets_consulted.push(rds.clone());
    }

    let rules_applied: Vec<String> = plan
        .semantic_rules
        .iter()
        .map(|r| r.rule_id.clone())
        .collect();
    let verified_by_stewards: Vec<String> = plan
        .semantic_rules
        .iter()
        .filter_map(|r| r.verified_by.clone())
        .collect();
    let all_rules_human_verified = !plan.semantic_rules.is_empty()
        && plan
            .semantic_rules
            .iter()
            .all(|r| r.epistemic_status == "verified");
    let min_rule_confidence = plan
        .semantic_rules
        .iter()
        .map(|r| r.confidence)
        .fold(1.0_f64, f64::min);

    let provenance = provenance_store.record_run(
        &plan.query_name,
        datasets_consulted,
        rules_applied,
        scaled_value,
        TrustSummary {
            all_rules_human_verified,
            min_rule_confidence,
            verified_by_stewards,
            pre_aggregation_applied: validation.pre_aggregation_rewrite_required,
            unit_conversion_applied: (validation.unit_scale_divisor - 1.0).abs() > 1e-6,
        },
    )?;

    Ok(QueryExecutionResult {
        validation,
        executed: true,
        computed_value: Some(scaled_value),
        rows_evaluated,
        provenance: Some(provenance),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeder::seed_validation_suite;
    use tempfile::tempdir;

    #[test]
    fn test_validate_blocks_trap_01_forbidden_join() {
        let dir = tempdir().expect("tempdir");
        seed_validation_suite(42, 80, dir.path()).expect("seed");
        let store = ProvenanceStore::default();

        let plan = SemanticQueryPlan {
            query_name: "trap_01_billing_marketing_join".to_string(),
            left_dataset: "billing_subscriptions".to_string(),
            left_parquet: dir
                .path()
                .join("billing_subscriptions.parquet")
                .to_string_lossy()
                .to_string(),
            left_join_field: Some("account_id".to_string()),
            right_dataset: Some("marketing_leads".to_string()),
            right_parquet: Some(
                dir.path()
                    .join("marketing_leads.parquet")
                    .to_string_lossy()
                    .to_string(),
            ),
            right_join_field: Some("account_id".to_string()),
            metric_dataset: "billing_subscriptions".to_string(),
            metric_field: "mrr_usd".to_string(),
            aggregation: "SUM".to_string(),
            semantic_rules: vec![SemanticRuleBinding {
                rule_id: "rule_trap_01_forbidden".to_string(),
                rule_type: "FORBIDDEN_JOIN".to_string(),
                left_dataset: "billing_subscriptions".to_string(),
                left_field: "account_id".to_string(),
                right_dataset: Some("marketing_leads".to_string()),
                right_field: Some("account_id".to_string()),
                cardinality: "1:1".to_string(),
                confidence: 1.0,
                epistemic_status: "rejected".to_string(),
                verified_by: Some("bob@polecatworks.io".to_string()),
                scale_divisor: 1.0,
            }],
            enforce_epistemic_gate: true,
        };

        let res = execute_query_plan(&plan, &store).expect("execute");
        assert!(!res.executed);
        assert_eq!(res.validation.status, "blocked_forbidden_join");
        assert_eq!(res.validation.blocked_forbidden_joins.len(), 1);
    }

    #[test]
    fn test_execute_prevents_trap_03_fanout_revenue_inflation() {
        let dir = tempdir().expect("tempdir");
        let manifest = seed_validation_suite(42, 80, dir.path()).expect("seed");
        let store = ProvenanceStore::default();

        // 1. Naive plan without N:M rule inflates revenue by ~2.5x
        let naive_plan = SemanticQueryPlan {
            query_name: "naive_items_payments_sum".to_string(),
            left_dataset: "order_items".to_string(),
            left_parquet: dir
                .path()
                .join("order_items.parquet")
                .to_string_lossy()
                .to_string(),
            left_join_field: Some("order_id".to_string()),
            right_dataset: Some("order_payments".to_string()),
            right_parquet: Some(
                dir.path()
                    .join("order_payments.parquet")
                    .to_string_lossy()
                    .to_string(),
            ),
            right_join_field: Some("order_id".to_string()),
            metric_dataset: "order_payments".to_string(),
            metric_field: "payment_amount_usd".to_string(),
            aggregation: "SUM".to_string(),
            semantic_rules: vec![],
            enforce_epistemic_gate: false,
        };
        let naive_res = execute_query_plan(&naive_plan, &store).expect("naive exec");
        assert_eq!(
            naive_res.computed_value,
            Some(manifest.naive_fanout_inflated_revenue_usd)
        );

        // 2. Semantic plan with N:M rule applies pre-aggregation rewrite and produces exact true revenue
        let safe_plan = SemanticQueryPlan {
            query_name: "safe_items_payments_sum".to_string(),
            semantic_rules: vec![SemanticRuleBinding {
                rule_id: "rule_trap_03_nm_fanout".to_string(),
                rule_type: "JOINABLE_WITH".to_string(),
                left_dataset: "order_items".to_string(),
                left_field: "order_id".to_string(),
                right_dataset: Some("order_payments".to_string()),
                right_field: Some("order_id".to_string()),
                cardinality: "N:M".to_string(),
                confidence: 0.95,
                epistemic_status: "verified".to_string(),
                verified_by: Some("alice@polecatworks.io".to_string()),
                scale_divisor: 1.0,
            }],
            enforce_epistemic_gate: true,
            ..naive_plan
        };
        let safe_res = execute_query_plan(&safe_plan, &store).expect("safe exec");
        assert!(safe_res.executed);
        assert!(safe_res.validation.pre_aggregation_rewrite_required);
        assert_eq!(
            safe_res.computed_value,
            Some(manifest.true_total_order_revenue_usd)
        );
        let prov = safe_res.provenance.expect("provenance run");
        assert!(prov.trust_summary.all_rules_human_verified);
        assert!(prov.trust_summary.pre_aggregation_applied);
    }

    #[test]
    fn test_execute_handles_trap_04_drift_gate_and_verified_unit_conversion() {
        let dir = tempdir().expect("tempdir");
        let manifest = seed_validation_suite(42, 80, dir.path()).expect("seed");
        let store = ProvenanceStore::default();

        // 1. Pre-HITL: Drifted rule halts at epistemic guardrail gate
        let drifted_plan = SemanticQueryPlan {
            query_name: "wave3_mean_payment".to_string(),
            left_dataset: "payment_events_wave3".to_string(),
            left_parquet: dir
                .path()
                .join("payment_events_wave3.parquet")
                .to_string_lossy()
                .to_string(),
            left_join_field: None,
            right_dataset: None,
            right_parquet: None,
            right_join_field: None,
            metric_dataset: "payment_events_wave3".to_string(),
            metric_field: "amount".to_string(),
            aggregation: "MEAN".to_string(),
            semantic_rules: vec![SemanticRuleBinding {
                rule_id: "drift_wave3_amount".to_string(),
                rule_type: "DRIFT_ALERT".to_string(),
                left_dataset: "payment_events_wave3".to_string(),
                left_field: "amount".to_string(),
                right_dataset: None,
                right_field: None,
                cardinality: "1:1".to_string(),
                confidence: 0.30,
                epistemic_status: "drifted".to_string(),
                verified_by: None,
                scale_divisor: 1.0,
            }],
            enforce_epistemic_gate: true,
        };
        let pre_res = execute_query_plan(&drifted_plan, &store).expect("pre-hitl");
        assert!(!pre_res.executed);
        assert!(pre_res.validation.requires_steward_review);
        assert_eq!(pre_res.validation.status, "requires_steward_review");

        // 2. Post-HITL: Steward certifies unit conversion (cents / 100.0 -> USD)
        let verified_plan = SemanticQueryPlan {
            semantic_rules: vec![SemanticRuleBinding {
                rule_id: "verified_wave3_cents_to_usd".to_string(),
                rule_type: "UNIT_CONVERSION".to_string(),
                left_dataset: "payment_events_wave3".to_string(),
                left_field: "amount".to_string(),
                right_dataset: None,
                right_field: None,
                cardinality: "1:1".to_string(),
                confidence: 1.0,
                epistemic_status: "verified".to_string(),
                verified_by: Some("steward@polecatworks.io".to_string()),
                scale_divisor: 100.0,
            }],
            ..drifted_plan
        };
        let post_res = execute_query_plan(&verified_plan, &store).expect("post-hitl");
        assert!(post_res.executed);
        assert_eq!(
            post_res.computed_value,
            Some(manifest.wave1_payment_mean_usd)
        );
        assert!(
            post_res
                .provenance
                .expect("prov")
                .trust_summary
                .unit_conversion_applied
        );
    }
}
