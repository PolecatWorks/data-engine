use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;

use arrow::array::{Array, Float64Array, Int64Array, StringArray};
use arrow::datatypes::DataType;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::ComputeConfig;

#[derive(Debug, Error)]
pub enum JoinProbeError {
    #[error("failed to open parquet file during join probe: {0}")]
    Io(#[from] std::io::Error),

    #[error("parquet error during join probe: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("arrow error during join probe: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("column '{column}' not found in dataset '{dataset}'")]
    ColumnNotFound { dataset: String, column: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JoinProbeResult {
    pub left_dataset: String,
    pub left_field: String,
    pub right_dataset: String,
    pub right_field: String,
    pub left_row_count: usize,
    pub right_row_count: usize,
    pub left_distinct_count: usize,
    pub right_distinct_count: usize,
    pub joined_row_count: usize,
    pub inclusion_ratio_left_in_right: f64,
    pub inclusion_ratio_right_in_left: f64,
    pub jaccard_similarity: f64,
    pub observed_cardinality: String,
    pub row_multiplication_factor: f64,
    pub fan_out_risk: bool,
    pub homonym_trap_warning: bool,
    pub confidence: f64,
    pub epistemic_status: String,
    pub recommended_action: String,
    pub explanation: String,
}

struct ExtractedColumnScan {
    row_count: usize,
    key_frequencies: HashMap<String, usize>,
    is_sequential_int_surrogate: bool,
    non_key_prefixes: HashSet<String>,
}

fn scan_parquet_column(
    dataset_name: &str,
    parquet_path: &Path,
    target_column: &str,
) -> Result<ExtractedColumnScan, JoinProbeError> {
    let file = File::open(parquet_path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();

    let target_idx = schema
        .fields()
        .iter()
        .position(|f| f.name() == target_column)
        .ok_or_else(|| JoinProbeError::ColumnNotFound {
            dataset: dataset_name.to_string(),
            column: target_column.to_string(),
        })?;

    let reader = builder.build()?;
    let mut row_count = 0_usize;
    let mut key_frequencies: HashMap<String, usize> = HashMap::new();
    let mut int_values: Vec<i64> = Vec::new();
    let mut non_key_prefixes: HashSet<String> = HashSet::new();

    for batch_res in reader {
        let batch = batch_res?;
        row_count += batch.num_rows();

        let col = batch.column(target_idx);
        match schema.field(target_idx).data_type() {
            DataType::Utf8 => {
                if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
                    for i in 0..arr.len() {
                        if !arr.is_null(i) {
                            *key_frequencies
                                .entry(arr.value(i).to_string())
                                .or_insert(0) += 1;
                        }
                    }
                }
            }
            DataType::Int64 => {
                if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
                    for i in 0..arr.len() {
                        if !arr.is_null(i) {
                            let val = arr.value(i);
                            *key_frequencies.entry(val.to_string()).or_insert(0) += 1;
                            int_values.push(val);
                        }
                    }
                }
            }
            DataType::Float64 => {
                if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
                    for i in 0..arr.len() {
                        if !arr.is_null(i) {
                            let val = format!("{:.4}", arr.value(i));
                            *key_frequencies.entry(val).or_insert(0) += 1;
                        }
                    }
                }
            }
            _ => {}
        }

        // Collect entity prefixes from non-key string columns to detect disjoint entity domains on surrogate keys
        for (idx, field) in schema.fields().iter().enumerate() {
            if idx == target_idx {
                continue;
            }
            if matches!(field.data_type(), DataType::Utf8) {
                if let Some(arr) = batch.column(idx).as_any().downcast_ref::<StringArray>() {
                    for i in 0..arr.len().min(25) {
                        if !arr.is_null(i) {
                            let s = arr.value(i);
                            if let Some((p1, rest)) = s.split_once('-') {
                                if let Some((p2, _)) = rest.split_once('-') {
                                    non_key_prefixes.insert(format!("{}-{}", p1, p2));
                                } else {
                                    non_key_prefixes.insert(p1.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let distinct_count = key_frequencies.len();
    let is_sequential_int_surrogate = if !int_values.is_empty() && int_values.len() == distinct_count
    {
        let min_i = *int_values.iter().min().unwrap_or(&0);
        let max_i = *int_values.iter().max().unwrap_or(&0);
        min_i == 1 && max_i == (distinct_count as i64)
    } else {
        false
    };

    Ok(ExtractedColumnScan {
        row_count,
        key_frequencies,
        is_sequential_int_surrogate,
        non_key_prefixes,
    })
}

pub fn probe_join(
    left_dataset: &str,
    left_parquet: &Path,
    left_field: &str,
    right_dataset: &str,
    right_parquet: &Path,
    right_field: &str,
    config: &ComputeConfig,
) -> Result<JoinProbeResult, JoinProbeError> {
    let left_scan = scan_parquet_column(left_dataset, left_parquet, left_field)?;
    let right_scan = scan_parquet_column(right_dataset, right_parquet, right_field)?;

    let left_distinct = left_scan.key_frequencies.len();
    let right_distinct = right_scan.key_frequencies.len();

    let mut intersection_distinct = 0_usize;
    let mut joined_row_count = 0_usize;
    let mut max_left_mult = 0_usize;
    let mut max_right_mult = 0_usize;

    for (key, l_count) in &left_scan.key_frequencies {
        if let Some(r_count) = right_scan.key_frequencies.get(key) {
            intersection_distinct += 1;
            joined_row_count += l_count * r_count;
            if *l_count > max_left_mult {
                max_left_mult = *l_count;
            }
            if *r_count > max_right_mult {
                max_right_mult = *r_count;
            }
        }
    }

    let union_distinct = left_distinct + right_distinct - intersection_distinct;
    let inclusion_left_in_right = if left_distinct == 0 {
        0.0
    } else {
        (intersection_distinct as f64) / (left_distinct as f64)
    };
    let inclusion_right_in_left = if right_distinct == 0 {
        0.0
    } else {
        (intersection_distinct as f64) / (right_distinct as f64)
    };
    let jaccard_similarity = if union_distinct == 0 {
        0.0
    } else {
        (intersection_distinct as f64) / (union_distinct as f64)
    };

    let observed_cardinality = if intersection_distinct == 0 {
        "disjoint".to_string()
    } else if max_left_mult <= 1 && max_right_mult <= 1 {
        "1:1".to_string()
    } else if max_left_mult > 1 && max_right_mult <= 1 {
        "N:1".to_string()
    } else if max_left_mult <= 1 && max_right_mult > 1 {
        "1:N".to_string()
    } else {
        "N:M".to_string()
    };

    let base_max_rows = left_scan.row_count.max(right_scan.row_count).max(1);
    let row_multiplication_factor =
        ((joined_row_count as f64) / (base_max_rows as f64) * 100.0).round() / 100.0;

    let fan_out_risk = observed_cardinality == "N:M" || row_multiplication_factor > 1.05;

    // Detect TRAP-01: Both columns are sequential integer surrogate keys (1..=N) with identical column names,
    // yet the two tables have non-overlapping entity code prefixes (e.g. TAX-ENT vs CMP-MKT).
    let prefixes_disjoint = !left_scan.non_key_prefixes.is_empty()
        && !right_scan.non_key_prefixes.is_empty()
        && left_scan
            .non_key_prefixes
            .is_disjoint(&right_scan.non_key_prefixes);
    let homonym_trap_warning = left_scan.is_sequential_int_surrogate
        && right_scan.is_sequential_int_surrogate
        && prefixes_disjoint;

    let max_inclusion = inclusion_left_in_right.max(inclusion_right_in_left);

    let (confidence, epistemic_status, recommended_action, explanation) =
        if max_inclusion < config.join_inclusion_threshold {
            (
                (max_inclusion * 100.0).round() / 100.0,
                "rejected".to_string(),
                "do_not_join".to_string(),
                format!(
                    "Max inclusion ratio ({:.2}) is below threshold ({:.2}); columns are not joinable.",
                    max_inclusion, config.join_inclusion_threshold
                ),
            )
        } else if homonym_trap_warning {
            (
                0.35,
                "pending_review".to_string(),
                "require_steward_verification".to_string(),
                format!(
                    "Homonym surrogate key collision warning: '{}.{}' and '{}.{}' both contain sequential integers (1..N), but their entity prefixes ({:?} vs {:?}) belong to disjoint domains.",
                    left_dataset,
                    left_field,
                    right_dataset,
                    right_field,
                    left_scan.non_key_prefixes,
                    right_scan.non_key_prefixes
                ),
            )
        } else if fan_out_risk {
            (
                0.65,
                "pending_review".to_string(),
                "pre_aggregate_before_join".to_string(),
                format!(
                    "Many-to-many (N:M) fan-out trap detected: joining '{}.{}' to '{}.{}' multiplies rows by {:.2}x ({} joined rows vs {} max source rows). Pre-aggregate before joining.",
                    left_dataset,
                    left_field,
                    right_dataset,
                    right_field,
                    row_multiplication_factor,
                    joined_row_count,
                    base_max_rows
                ),
            )
        } else {
            (
                (max_inclusion * 100.0).round() / 100.0,
                "empirically_tested".to_string(),
                "sync_joinable_with".to_string(),
                format!(
                    "Verified {} join relationship between '{}.{}' and '{}.{}' with {:.1}% inclusion overlap.",
                    observed_cardinality,
                    left_dataset,
                    left_field,
                    right_dataset,
                    right_field,
                    max_inclusion * 100.0
                ),
            )
        };

    Ok(JoinProbeResult {
        left_dataset: left_dataset.to_string(),
        left_field: left_field.to_string(),
        right_dataset: right_dataset.to_string(),
        right_field: right_field.to_string(),
        left_row_count: left_scan.row_count,
        right_row_count: right_scan.row_count,
        left_distinct_count: left_distinct,
        right_distinct_count: right_distinct,
        joined_row_count,
        inclusion_ratio_left_in_right: (inclusion_left_in_right * 1000.0).round() / 1000.0,
        inclusion_ratio_right_in_left: (inclusion_right_in_left * 1000.0).round() / 1000.0,
        jaccard_similarity: (jaccard_similarity * 1000.0).round() / 1000.0,
        observed_cardinality,
        row_multiplication_factor,
        fan_out_risk,
        homonym_trap_warning,
        confidence,
        epistemic_status,
        recommended_action,
        explanation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeder::seed_validation_suite;
    use tempfile::tempdir;

    fn default_compute_config() -> ComputeConfig {
        ComputeConfig {
            query_timeout_secs: 30,
            max_preview_rows: 100,
            join_inclusion_threshold: 0.80,
        }
    }

    #[test]
    fn test_probe_trap_02_discovers_cryptic_synonym_join() {
        let dir = tempdir().expect("tempdir");
        seed_validation_suite(42, 100, dir.path()).expect("seed");
        let cfg = default_compute_config();

        let res = probe_join(
            "sales_orders",
            &dir.path().join("sales_orders.parquet"),
            "cust_ref_key",
            "crm_customers",
            &dir.path().join("crm_customers.parquet"),
            "party_uuid",
            &cfg,
        )
        .expect("probe TRAP-02");

        assert!(res.inclusion_ratio_left_in_right >= 0.95);
        assert_eq!(res.inclusion_ratio_right_in_left, 1.0);
        assert_eq!(res.observed_cardinality, "N:1");
        assert!(!res.fan_out_risk);
        assert!(!res.homonym_trap_warning);
        assert_eq!(res.epistemic_status, "empirically_tested");
        assert_eq!(res.recommended_action, "sync_joinable_with");
    }

    #[test]
    fn test_probe_trap_03_detects_nm_fanout_inflation() {
        let dir = tempdir().expect("tempdir");
        seed_validation_suite(42, 100, dir.path()).expect("seed");
        let cfg = default_compute_config();

        let res = probe_join(
            "order_items",
            &dir.path().join("order_items.parquet"),
            "order_id",
            "order_payments",
            &dir.path().join("order_payments.parquet"),
            "order_id",
            &cfg,
        )
        .expect("probe TRAP-03");

        assert_eq!(res.observed_cardinality, "N:M");
        assert!(res.fan_out_risk);
        assert!(res.row_multiplication_factor >= 1.9);
        assert_eq!(res.epistemic_status, "pending_review");
        assert_eq!(res.recommended_action, "pre_aggregate_before_join");
    }

    #[test]
    fn test_probe_trap_01_flags_homonym_surrogate_chasm() {
        let dir = tempdir().expect("tempdir");
        seed_validation_suite(42, 100, dir.path()).expect("seed");
        let cfg = default_compute_config();

        let res = probe_join(
            "billing_subscriptions",
            &dir.path().join("billing_subscriptions.parquet"),
            "account_id",
            "marketing_leads",
            &dir.path().join("marketing_leads.parquet"),
            "account_id",
            &cfg,
        )
        .expect("probe TRAP-01");

        assert!(res.homonym_trap_warning);
        assert_eq!(res.epistemic_status, "pending_review");
        assert_eq!(res.recommended_action, "require_steward_verification");
    }
}
