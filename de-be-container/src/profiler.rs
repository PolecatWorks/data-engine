use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;

use arrow::array::{Array, Float64Array, Int64Array, StringArray};
use arrow::datatypes::DataType;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::ProfilerConfig;

#[derive(Debug, Error)]
pub enum ProfilerError {
    #[error("failed to open parquet file: {0}")]
    Io(#[from] std::io::Error),

    #[error("parquet reader error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("arrow error during profiling: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NumericStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub is_sequential_integer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldProfile {
    pub field_name: String,
    pub arrow_data_type: String,
    pub null_count: usize,
    pub null_ratio: f64,
    pub distinct_count: usize,
    pub uniqueness_ratio: f64,
    pub top_k_values: Vec<String>,
    pub dominant_prefix: Option<String>,
    pub minhash_signature: Vec<u64>,
    pub numeric_stats: Option<NumericStats>,
    pub inferred_semantic_domain: String,
    pub epistemic_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriftFinding {
    pub drift_type: String,
    pub field_name: String,
    pub baseline_mean: Option<f64>,
    pub current_mean: Option<f64>,
    pub scale_ratio: Option<f64>,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatasetProfile {
    pub dataset_name: String,
    pub row_count: usize,
    pub schema_fingerprint_sha256: String,
    pub candidate_primary_keys: Vec<String>,
    pub epistemic_status: String,
    pub drift_findings: Vec<DriftFinding>,
    pub fields: Vec<FieldProfile>,
}

fn fnv1a_hash(value: &str, seed_idx: usize) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325_u64
        .wrapping_add((seed_idx as u64).wrapping_mul(0x9E3779B97F4A7C15));
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn compute_minhash(distinct_values: &HashSet<String>, num_hashes: usize) -> Vec<u64> {
    if distinct_values.is_empty() {
        return vec![0; num_hashes];
    }
    let mut sig = vec![u64::MAX; num_hashes];
    for val in distinct_values {
        for (i, slot) in sig.iter_mut().enumerate() {
            let h = fnv1a_hash(val, i);
            if h < *slot {
                *slot = h;
            }
        }
    }
    sig
}

fn infer_dominant_prefix(values: &HashSet<String>) -> Option<String> {
    let mut prefix_counts: HashMap<String, usize> = HashMap::new();
    for val in values {
        if let Some((first, _rest)) = val.split_once('-') {
            *prefix_counts.entry(first.to_string()).or_insert(0) += 1;
        }
    }
    let total = values.len();
    if total == 0 {
        return None;
    }
    prefix_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .and_then(|(prefix, count)| {
            if (count as f64) / (total as f64) >= 0.8 {
                Some(prefix)
            } else {
                None
            }
        })
}

fn infer_semantic_domain(
    field_name: &str,
    data_type: &DataType,
    uniqueness_ratio: f64,
    distinct_count: usize,
) -> String {
    let lower = field_name.to_lowercase();
    if lower.ends_with("_id")
        || lower.ends_with("_uuid")
        || lower.ends_with("_key")
        || (uniqueness_ratio > 0.9 && distinct_count > 10)
    {
        return "identifier".to_string();
    }
    if matches!(data_type, DataType::Float64)
        || lower.contains("amount")
        || lower.contains("price")
        || lower.contains("total")
        || lower.contains("mrr")
    {
        return "currency_amount".to_string();
    }
    if distinct_count <= 20 {
        return "categorical".to_string();
    }
    "text".to_string()
}

pub fn profile_parquet_dataset(
    dataset_name: &str,
    parquet_path: &Path,
    config: &ProfilerConfig,
    baseline: Option<&DatasetProfile>,
) -> Result<DatasetProfile, ProfilerError> {
    let file = File::open(parquet_path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();

    let mut hasher = Sha256::new();
    for field in schema.fields() {
        hasher.update(format!("{}:{:?};", field.name(), field.data_type()).as_bytes());
    }
    let schema_fingerprint_sha256 = format!("{:x}", hasher.finalize());

    let reader = builder.build()?;
    let mut batches = Vec::new();
    let mut total_rows = 0_usize;

    for batch_res in reader {
        let batch = batch_res?;
        total_rows += batch.num_rows();
        batches.push(batch);
        if total_rows >= config.max_sample_rows {
            break;
        }
    }

    let mut fields = Vec::with_capacity(schema.fields().len());
    let mut candidate_primary_keys = Vec::new();

    for (col_idx, field) in schema.fields().iter().enumerate() {
        let mut null_count = 0_usize;
        let mut value_counts: HashMap<String, usize> = HashMap::new();
        let mut distinct_set: HashSet<String> = HashSet::new();
        let mut numeric_vals: Vec<f64> = Vec::new();
        let mut int_vals: Vec<i64> = Vec::new();

        for batch in &batches {
            let col = batch.column(col_idx);
            null_count += col.null_count();

            match field.data_type() {
                DataType::Utf8 => {
                    if let Some(arr) = col.as_any().downcast_ref::<StringArray>() {
                        for i in 0..arr.len() {
                            if !arr.is_null(i) {
                                let v = arr.value(i).to_string();
                                *value_counts.entry(v.clone()).or_insert(0) += 1;
                                distinct_set.insert(v);
                            }
                        }
                    }
                }
                DataType::Int64 => {
                    if let Some(arr) = col.as_any().downcast_ref::<Int64Array>() {
                        for i in 0..arr.len() {
                            if !arr.is_null(i) {
                                let num = arr.value(i);
                                let v = num.to_string();
                                *value_counts.entry(v.clone()).or_insert(0) += 1;
                                distinct_set.insert(v);
                                numeric_vals.push(num as f64);
                                int_vals.push(num);
                            }
                        }
                    }
                }
                DataType::Float64 => {
                    if let Some(arr) = col.as_any().downcast_ref::<Float64Array>() {
                        for i in 0..arr.len() {
                            if !arr.is_null(i) {
                                let num = arr.value(i);
                                let v = format!("{:.2}", num);
                                *value_counts.entry(v.clone()).or_insert(0) += 1;
                                distinct_set.insert(v);
                                numeric_vals.push(num);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let distinct_count = distinct_set.len();
        let null_ratio = if total_rows == 0 {
            0.0
        } else {
            (null_count as f64) / (total_rows as f64)
        };
        let uniqueness_ratio = if total_rows == 0 {
            0.0
        } else {
            (distinct_count as f64) / (total_rows as f64)
        };

        let mut sorted_top: Vec<(String, usize)> = value_counts.into_iter().collect();
        sorted_top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let top_k_values: Vec<String> = sorted_top
            .into_iter()
            .take(config.top_k_values)
            .map(|(k, _)| k)
            .collect();

        let numeric_stats = if !numeric_vals.is_empty() {
            let min = numeric_vals
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min);
            let max = numeric_vals
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let mean = numeric_vals.iter().sum::<f64>() / (numeric_vals.len() as f64);

            let is_sequential_integer = if !int_vals.is_empty() && int_vals.len() == distinct_count
            {
                let min_i = *int_vals.iter().min().unwrap_or(&0);
                let max_i = *int_vals.iter().max().unwrap_or(&0);
                min_i == 1 && max_i == (int_vals.len() as i64)
            } else {
                false
            };

            Some(NumericStats {
                min,
                max,
                mean,
                is_sequential_integer,
            })
        } else {
            None
        };

        let dominant_prefix = infer_dominant_prefix(&distinct_set);
        let minhash_signature = compute_minhash(&distinct_set, config.minhash_num_hashes);
        let inferred_semantic_domain = infer_semantic_domain(
            field.name(),
            field.data_type(),
            uniqueness_ratio,
            distinct_count,
        );

        let lower_name = field.name().to_lowercase();
        let is_id_named = lower_name.ends_with("_id")
            || lower_name.ends_with("_uuid")
            || lower_name.ends_with("_key")
            || lower_name == "id";

        if total_rows > 0 && null_count == 0 && distinct_count == total_rows && is_id_named {
            candidate_primary_keys.push(field.name().clone());
        }

        fields.push(FieldProfile {
            field_name: field.name().clone(),
            arrow_data_type: format!("{:?}", field.data_type()),
            null_count,
            null_ratio,
            distinct_count,
            uniqueness_ratio,
            top_k_values,
            dominant_prefix,
            minhash_signature,
            numeric_stats,
            inferred_semantic_domain,
            epistemic_status: "discovered".to_string(),
        });
    }

    let mut profile = DatasetProfile {
        dataset_name: dataset_name.to_string(),
        row_count: total_rows,
        schema_fingerprint_sha256,
        candidate_primary_keys,
        epistemic_status: "discovered".to_string(),
        drift_findings: Vec::new(),
        fields,
    };

    if let Some(base) = baseline {
        apply_drift_comparison(&mut profile, base, config.drift_relative_mean_threshold);
    }

    Ok(profile)
}

fn apply_drift_comparison(
    current: &mut DatasetProfile,
    baseline: &DatasetProfile,
    relative_mean_threshold: f64,
) {
    let base_map: HashMap<&str, &FieldProfile> = baseline
        .fields
        .iter()
        .map(|f| (f.field_name.as_str(), f))
        .collect();
    let curr_names: HashSet<&str> = current
        .fields
        .iter()
        .map(|f| f.field_name.as_str())
        .collect();

    let added_columns: Vec<String> = current
        .fields
        .iter()
        .filter(|f| !base_map.contains_key(f.field_name.as_str()))
        .map(|f| f.field_name.clone())
        .collect();

    for base_field in &baseline.fields {
        if !curr_names.contains(base_field.field_name.as_str()) {
            let replacement_hint = if !added_columns.is_empty() {
                format!(" (candidate replacement: '{}')", added_columns.join(", "))
            } else {
                String::new()
            };
            current.drift_findings.push(DriftFinding {
                drift_type: "column_removed_or_renamed".to_string(),
                field_name: base_field.field_name.clone(),
                baseline_mean: None,
                current_mean: None,
                scale_ratio: None,
                details: format!(
                    "Field '{}' present in baseline is missing in current snapshot{}",
                    base_field.field_name, replacement_hint
                ),
            });
        }
    }

    for curr_field in &mut current.fields {
        if let Some(base_field) = base_map.get(curr_field.field_name.as_str()) {
            if let (Some(base_num), Some(curr_num)) =
                (&base_field.numeric_stats, &curr_field.numeric_stats)
            {
                if base_num.mean.abs() > 1e-6 && curr_num.mean.abs() > 1e-6 {
                    let ratio = if curr_num.mean.abs() >= base_num.mean.abs() {
                        curr_num.mean.abs() / base_num.mean.abs()
                    } else {
                        base_num.mean.abs() / curr_num.mean.abs()
                    };

                    if ratio >= relative_mean_threshold {
                        curr_field.epistemic_status = "drifted".to_string();
                        current.drift_findings.push(DriftFinding {
                            drift_type: "numeric_scale_shift".to_string(),
                            field_name: curr_field.field_name.clone(),
                            baseline_mean: Some(base_num.mean),
                            current_mean: Some(curr_num.mean),
                            scale_ratio: Some((ratio * 100.0).round() / 100.0),
                            details: format!(
                                "Numeric mean shifted by {:.2}x (exceeds threshold {:.2}x); possible unit change (e.g. USD to cents)",
                                ratio, relative_mean_threshold
                            ),
                        });
                    }
                }
            }
        } else {
            curr_field.epistemic_status = "drifted".to_string();
        }
    }

    if !current.drift_findings.is_empty()
        || current.schema_fingerprint_sha256 != baseline.schema_fingerprint_sha256
    {
        current.epistemic_status = "drifted".to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeder::seed_validation_suite;
    use tempfile::tempdir;

    fn default_profiler_config() -> ProfilerConfig {
        ProfilerConfig {
            max_sample_rows: 100_000,
            top_k_values: 5,
            minhash_num_hashes: 64,
            drift_relative_mean_threshold: 5.0,
        }
    }

    #[test]
    fn test_profile_seeded_crm_and_orders_tables() {
        let dir = tempdir().expect("tempdir");
        seed_validation_suite(42, 100, dir.path()).expect("seed");
        let cfg = default_profiler_config();

        let crm_profile = profile_parquet_dataset(
            "crm_customers",
            &dir.path().join("crm_customers.parquet"),
            &cfg,
            None,
        )
        .expect("profile crm");

        assert_eq!(crm_profile.row_count, 100);
        assert_eq!(
            crm_profile.candidate_primary_keys,
            vec!["party_uuid".to_string()]
        );
        assert_eq!(crm_profile.epistemic_status, "discovered");
        let party_field = crm_profile
            .fields
            .iter()
            .find(|f| f.field_name == "party_uuid")
            .expect("party_uuid field");
        assert_eq!(party_field.null_ratio, 0.0);
        assert_eq!(party_field.distinct_count, 100);
        assert_eq!(party_field.minhash_signature.len(), 64);
        assert_eq!(party_field.dominant_prefix, Some("PTY".to_string()));
    }

    #[test]
    fn test_detect_trap_04_temporal_schema_and_unit_drift() {
        let dir = tempdir().expect("tempdir");
        seed_validation_suite(42, 100, dir.path()).expect("seed");
        let cfg = default_profiler_config();

        let wave1 = profile_parquet_dataset(
            "payment_events_wave1",
            &dir.path().join("payment_events_wave1.parquet"),
            &cfg,
            None,
        )
        .expect("profile wave1");

        let wave3 = profile_parquet_dataset(
            "payment_events_wave3",
            &dir.path().join("payment_events_wave3.parquet"),
            &cfg,
            Some(&wave1),
        )
        .expect("profile wave3 with baseline");

        assert_eq!(wave3.epistemic_status, "drifted");
        assert!(
            wave3
                .drift_findings
                .iter()
                .any(|d| d.drift_type == "column_removed_or_renamed" && d.field_name == "user_id"),
            "expected column rename drift finding for user_id -> client_id"
        );
        let scale_finding = wave3
            .drift_findings
            .iter()
            .find(|d| d.drift_type == "numeric_scale_shift" && d.field_name == "amount")
            .expect("expected numeric scale shift finding for amount");
        assert_eq!(scale_finding.scale_ratio, Some(100.0));

        let amount_field = wave3
            .fields
            .iter()
            .find(|f| f.field_name == "amount")
            .expect("amount field");
        assert_eq!(amount_field.epistemic_status, "drifted");
    }
}
