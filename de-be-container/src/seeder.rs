use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow::array::{Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SeederError {
    #[error("failed to create seeder output directory: {0}")]
    Io(#[from] std::io::Error),

    #[error("arrow error during seeding: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("parquet writer error during seeding: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("manifest serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlantedTrapSpec {
    pub trap_id: String,
    pub category: String,
    pub left_dataset: String,
    pub left_field: String,
    pub right_dataset: String,
    pub right_field: String,
    pub expected_action: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GroundTruthManifest {
    pub seed: u64,
    pub base_row_count: usize,
    pub generated_files: Vec<String>,
    pub true_total_order_revenue_usd: f64,
    pub naive_fanout_inflated_revenue_usd: f64,
    pub expected_fanout_multiplier: f64,
    pub wave1_payment_mean_usd: f64,
    pub wave3_payment_mean_cents: f64,
    pub planted_traps: Vec<PlantedTrapSpec>,
}

/// Simple deterministic SplitMix64 RNG so our test suite has zero platform-specific RNG variance.
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn next_usize(&mut self, bound: usize) -> usize {
        (self.next_u64() as usize) % bound
    }
}

fn write_parquet_batch(path: &Path, batch: &RecordBatch) -> Result<(), SeederError> {
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None)?;
    writer.write(batch)?;
    writer.close()?;
    Ok(())
}

pub fn seed_validation_suite(
    seed: u64,
    base_row_count: usize,
    output_dir: &Path,
) -> Result<GroundTruthManifest, SeederError> {
    fs::create_dir_all(output_dir)?;
    let mut rng = DeterministicRng::new(seed);

    let regions = ["EMEA", "AMER", "APAC", "LATAM"];
    let tiers = ["Enterprise", "MidMarket", "Growth"];
    let statuses = ["completed", "completed", "completed", "shipped"];

    // 1. crm_customers.parquet
    let mut party_uuids = Vec::with_capacity(base_row_count);
    let mut customer_names = Vec::with_capacity(base_row_count);
    let mut customer_regions = Vec::with_capacity(base_row_count);
    let mut customer_tiers = Vec::with_capacity(base_row_count);

    for idx in 0..base_row_count {
        let uid = format!("PTY-{:04X}-{:06}", (seed & 0xFFFF), idx + 1);
        party_uuids.push(uid);
        customer_names.push(format!("Acme Corp {:04}", idx + 1));
        customer_regions.push(regions[rng.next_usize(regions.len())].to_string());
        customer_tiers.push(tiers[rng.next_usize(tiers.len())].to_string());
    }

    let crm_schema = Arc::new(Schema::new(vec![
        Field::new("party_uuid", DataType::Utf8, false),
        Field::new("customer_name", DataType::Utf8, false),
        Field::new("region", DataType::Utf8, false),
        Field::new("tier", DataType::Utf8, false),
    ]));
    let crm_batch = RecordBatch::try_new(
        crm_schema,
        vec![
            Arc::new(StringArray::from(party_uuids.clone())),
            Arc::new(StringArray::from(customer_names)),
            Arc::new(StringArray::from(customer_regions)),
            Arc::new(StringArray::from(customer_tiers)),
        ],
    )?;
    write_parquet_batch(&output_dir.join("crm_customers.parquet"), &crm_batch)?;

    // 2. sales_orders.parquet (TRAP-02: cust_ref_key -> crm_customers.party_uuid with >= 97% overlap)
    // Also drives TRAP-03: order_items (2..3 rows/order) x order_payments (2..3 installments/order)
    let order_count = base_row_count * 2;
    let mut order_ids = Vec::with_capacity(order_count);
    let mut cust_ref_keys = Vec::with_capacity(order_count);
    let mut order_totals = Vec::with_capacity(order_count);
    let mut order_statuses = Vec::with_capacity(order_count);

    let mut item_ids = Vec::new();
    let mut item_order_ids = Vec::new();
    let mut item_skus = Vec::new();
    let mut item_prices = Vec::new();

    let mut payment_ids = Vec::new();
    let mut payment_order_ids = Vec::new();
    let mut payment_seqs = Vec::new();
    let mut payment_amounts = Vec::new();

    let mut true_total_order_revenue_usd = 0.0_f64;
    let mut naive_fanout_inflated_revenue_usd = 0.0_f64;

    for ord_idx in 0..order_count {
        let oid = format!("ORD-{:06}", ord_idx + 1);
        // Ensure every customer is referenced and ~98.5% of orders map to known party_uuids
        let cust_key = if ord_idx < base_row_count {
            party_uuids[ord_idx].clone()
        } else if ord_idx % 60 == 0 {
            format!("PTY-GHOST-{:06}", ord_idx)
        } else {
            party_uuids[rng.next_usize(base_row_count)].clone()
        };

        // 2 or 3 line items per order
        let num_items = if ord_idx % 2 == 0 { 2 } else { 3 };
        let mut this_order_total = 0.0_f64;
        for i in 0..num_items {
            let price = 40.0 + ((rng.next_usize(160) as f64) * 1.5);
            this_order_total += price;
            item_ids.push(format!("ITM-{:06}-{}", ord_idx + 1, i + 1));
            item_order_ids.push(oid.clone());
            item_skus.push(format!("SKU-{:03}", (i + ord_idx) % 50 + 1));
            item_prices.push(price);
        }

        // 2 payment installments per order summing to this_order_total
        let num_payments = 2_usize;
        let installment_amt = this_order_total / (num_payments as f64);
        for p in 0..num_payments {
            payment_ids.push(format!("PAY-{:06}-{}", ord_idx + 1, p + 1));
            payment_order_ids.push(oid.clone());
            payment_seqs.push((p + 1) as i64);
            payment_amounts.push(installment_amt);
        }

        // When order_items (num_items rows) is joined directly to order_payments (num_payments rows)
        // on order_id without pre-aggregation, each payment row is repeated num_items times!
        true_total_order_revenue_usd += this_order_total;
        naive_fanout_inflated_revenue_usd += this_order_total * (num_items as f64);

        order_ids.push(oid);
        cust_ref_keys.push(cust_key);
        order_totals.push(this_order_total);
        order_statuses.push(statuses[ord_idx % statuses.len()].to_string());
    }

    let orders_schema = Arc::new(Schema::new(vec![
        Field::new("order_id", DataType::Utf8, false),
        Field::new("cust_ref_key", DataType::Utf8, false),
        Field::new("order_total_usd", DataType::Float64, false),
        Field::new("status", DataType::Utf8, false),
    ]));
    let orders_batch = RecordBatch::try_new(
        orders_schema,
        vec![
            Arc::new(StringArray::from(order_ids)),
            Arc::new(StringArray::from(cust_ref_keys)),
            Arc::new(Float64Array::from(order_totals)),
            Arc::new(StringArray::from(order_statuses)),
        ],
    )?;
    write_parquet_batch(&output_dir.join("sales_orders.parquet"), &orders_batch)?;

    // 3. order_items.parquet
    let items_schema = Arc::new(Schema::new(vec![
        Field::new("item_id", DataType::Utf8, false),
        Field::new("order_id", DataType::Utf8, false),
        Field::new("sku", DataType::Utf8, false),
        Field::new("item_price_usd", DataType::Float64, false),
    ]));
    let items_batch = RecordBatch::try_new(
        items_schema,
        vec![
            Arc::new(StringArray::from(item_ids)),
            Arc::new(StringArray::from(item_order_ids)),
            Arc::new(StringArray::from(item_skus)),
            Arc::new(Float64Array::from(item_prices)),
        ],
    )?;
    write_parquet_batch(&output_dir.join("order_items.parquet"), &items_batch)?;

    // 4. order_payments.parquet
    let payments_schema = Arc::new(Schema::new(vec![
        Field::new("payment_id", DataType::Utf8, false),
        Field::new("order_id", DataType::Utf8, false),
        Field::new("installment_seq", DataType::Int64, false),
        Field::new("payment_amount_usd", DataType::Float64, false),
    ]));
    let payments_batch = RecordBatch::try_new(
        payments_schema,
        vec![
            Arc::new(StringArray::from(payment_ids)),
            Arc::new(StringArray::from(payment_order_ids)),
            Arc::new(Int64Array::from(payment_seqs)),
            Arc::new(Float64Array::from(payment_amounts)),
        ],
    )?;
    write_parquet_batch(&output_dir.join("order_payments.parquet"), &payments_batch)?;

    // 5 & 6. TRAP-01: billing_subscriptions.parquet vs marketing_leads.parquet
    // Both have sequential surrogate `account_id` (1..=base_row_count), creating a 100% value overlap
    // on an auto-increment integer column that actually represents two completely disjoint namespaces!
    let mut billing_account_ids = Vec::with_capacity(base_row_count);
    let mut billing_tax_ids = Vec::with_capacity(base_row_count);
    let mut billing_mrr = Vec::with_capacity(base_row_count);

    let mut marketing_account_ids = Vec::with_capacity(base_row_count);
    let mut marketing_campaigns = Vec::with_capacity(base_row_count);
    let mut marketing_click_ids = Vec::with_capacity(base_row_count);

    for idx in 0..base_row_count {
        let surrogate_id = (idx + 1) as i64;
        billing_account_ids.push(surrogate_id);
        billing_tax_ids.push(format!("TAX-ENT-{:05}", idx + 1));
        billing_mrr.push(500.0 + (idx as f64) * 15.0);

        marketing_account_ids.push(surrogate_id);
        marketing_campaigns.push(format!("CMP-MKT-{:05}", idx + 1));
        marketing_click_ids.push(format!("CLK-AD-{:06}", idx + 1));
    }

    let billing_schema = Arc::new(Schema::new(vec![
        Field::new("account_id", DataType::Int64, false),
        Field::new("billing_tax_id", DataType::Utf8, false),
        Field::new("mrr_usd", DataType::Float64, false),
    ]));
    let billing_batch = RecordBatch::try_new(
        billing_schema,
        vec![
            Arc::new(Int64Array::from(billing_account_ids)),
            Arc::new(StringArray::from(billing_tax_ids)),
            Arc::new(Float64Array::from(billing_mrr)),
        ],
    )?;
    write_parquet_batch(
        &output_dir.join("billing_subscriptions.parquet"),
        &billing_batch,
    )?;

    let marketing_schema = Arc::new(Schema::new(vec![
        Field::new("account_id", DataType::Int64, false),
        Field::new("lead_campaign_code", DataType::Utf8, false),
        Field::new("ad_click_id", DataType::Utf8, false),
    ]));
    let marketing_batch = RecordBatch::try_new(
        marketing_schema,
        vec![
            Arc::new(Int64Array::from(marketing_account_ids)),
            Arc::new(StringArray::from(marketing_campaigns)),
            Arc::new(StringArray::from(marketing_click_ids)),
        ],
    )?;
    write_parquet_batch(
        &output_dir.join("marketing_leads.parquet"),
        &marketing_batch,
    )?;

    // 7 & 8. TRAP-04: Temporal Schema & 100x Unit Drift (payment_events_wave1 vs payment_events_wave3)
    let mut w1_event_ids = Vec::with_capacity(base_row_count);
    let mut w1_user_ids = Vec::with_capacity(base_row_count);
    let mut w1_amounts_usd = Vec::with_capacity(base_row_count);

    let mut w3_event_ids = Vec::with_capacity(base_row_count);
    let mut w3_client_ids = Vec::with_capacity(base_row_count);
    let mut w3_amounts_cents = Vec::with_capacity(base_row_count);

    for idx in 0..base_row_count {
        let amt_usd = 100.0 + ((idx % 41) as f64); // Mean ~120.0 USD
        w1_event_ids.push(format!("EVT-W1-{:05}", idx + 1));
        w1_user_ids.push(party_uuids[idx].clone());
        w1_amounts_usd.push(amt_usd);

        w3_event_ids.push(format!("EVT-W3-{:05}", idx + 1));
        w3_client_ids.push(party_uuids[idx].clone());
        w3_amounts_cents.push(amt_usd * 100.0); // 100x shift to cents (~12000.0)
    }

    let wave1_mean: f64 = w1_amounts_usd.iter().sum::<f64>() / (base_row_count as f64);
    let wave3_mean: f64 = w3_amounts_cents.iter().sum::<f64>() / (base_row_count as f64);

    let w1_schema = Arc::new(Schema::new(vec![
        Field::new("event_id", DataType::Utf8, false),
        Field::new("user_id", DataType::Utf8, false),
        Field::new("amount", DataType::Float64, false),
    ]));
    let w1_batch = RecordBatch::try_new(
        w1_schema,
        vec![
            Arc::new(StringArray::from(w1_event_ids)),
            Arc::new(StringArray::from(w1_user_ids)),
            Arc::new(Float64Array::from(w1_amounts_usd)),
        ],
    )?;
    write_parquet_batch(
        &output_dir.join("payment_events_wave1.parquet"),
        &w1_batch,
    )?;

    let w3_schema = Arc::new(Schema::new(vec![
        Field::new("event_id", DataType::Utf8, false),
        Field::new("client_id", DataType::Utf8, false),
        Field::new("amount", DataType::Float64, false),
    ]));
    let w3_batch = RecordBatch::try_new(
        w3_schema,
        vec![
            Arc::new(StringArray::from(w3_event_ids)),
            Arc::new(StringArray::from(w3_client_ids)),
            Arc::new(Float64Array::from(w3_amounts_cents)),
        ],
    )?;
    write_parquet_batch(
        &output_dir.join("payment_events_wave3.parquet"),
        &w3_batch,
    )?;

    let expected_fanout_multiplier =
        (naive_fanout_inflated_revenue_usd / true_total_order_revenue_usd * 100.0).round() / 100.0;

    let manifest = GroundTruthManifest {
        seed,
        base_row_count,
        generated_files: vec![
            "crm_customers.parquet".to_string(),
            "sales_orders.parquet".to_string(),
            "order_items.parquet".to_string(),
            "order_payments.parquet".to_string(),
            "billing_subscriptions.parquet".to_string(),
            "marketing_leads.parquet".to_string(),
            "payment_events_wave1.parquet".to_string(),
            "payment_events_wave3.parquet".to_string(),
        ],
        true_total_order_revenue_usd: (true_total_order_revenue_usd * 100.0).round() / 100.0,
        naive_fanout_inflated_revenue_usd: (naive_fanout_inflated_revenue_usd * 100.0).round()
            / 100.0,
        expected_fanout_multiplier,
        wave1_payment_mean_usd: (wave1_mean * 100.0).round() / 100.0,
        wave3_payment_mean_cents: (wave3_mean * 100.0).round() / 100.0,
        planted_traps: vec![
            PlantedTrapSpec {
                trap_id: "TRAP-01".to_string(),
                category: "homonym_chasm".to_string(),
                left_dataset: "billing_subscriptions".to_string(),
                left_field: "account_id".to_string(),
                right_dataset: "marketing_leads".to_string(),
                right_field: "account_id".to_string(),
                expected_action: "require_steward_verification_then_forbidden_join".to_string(),
                description: "Both tables use sequential integer surrogate account_id (1..N) for unrelated entity domains (Billing Tax Accounts vs Ad Marketing Leads).".to_string(),
            },
            PlantedTrapSpec {
                trap_id: "TRAP-02".to_string(),
                category: "cryptic_synonym_fk".to_string(),
                left_dataset: "sales_orders".to_string(),
                left_field: "cust_ref_key".to_string(),
                right_dataset: "crm_customers".to_string(),
                right_field: "party_uuid".to_string(),
                expected_action: "sync_joinable_with".to_string(),
                description: "Zero column-name similarity, but >98% value inclusion ratio and clean N:1 cardinality.".to_string(),
            },
            PlantedTrapSpec {
                trap_id: "TRAP-03".to_string(),
                category: "nm_fanout_inflation".to_string(),
                left_dataset: "order_items".to_string(),
                left_field: "order_id".to_string(),
                right_dataset: "order_payments".to_string(),
                right_field: "order_id".to_string(),
                expected_action: "pre_aggregate_before_join".to_string(),
                description: "Joining multi-row order_items to multi-row order_payments on order_id inflates payment revenue by ~2.5x.".to_string(),
            },
            PlantedTrapSpec {
                trap_id: "TRAP-04".to_string(),
                category: "temporal_schema_and_unit_drift".to_string(),
                left_dataset: "payment_events_wave1".to_string(),
                left_field: "amount".to_string(),
                right_dataset: "payment_events_wave3".to_string(),
                right_field: "amount".to_string(),
                expected_action: "mark_drifted_and_require_unit_conversion".to_string(),
                description: "Wave 3 renames user_id -> client_id and shifts amount from USD (~120.0) to integer cents (~12000.0, 100x scale shift).".to_string(),
            },
        ],
    };

    let manifest_path: PathBuf = output_dir.join("ground_truth_manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    fs::write(manifest_path, manifest_json)?;

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_seed_validation_suite_creates_all_parquet_files_and_manifest() {
        let dir = tempdir().expect("create temp dir");
        let manifest =
            seed_validation_suite(42, 100, dir.path()).expect("seed validation suite succeeds");

        assert_eq!(manifest.generated_files.len(), 8);
        for file_name in &manifest.generated_files {
            let p = dir.path().join(file_name);
            assert!(p.exists(), "expected generated file {:?} to exist", p);
            let meta = fs::metadata(&p).expect("file metadata");
            assert!(meta.len() > 100, "expected non-empty parquet file {:?}", p);
        }
        assert!(dir.path().join("ground_truth_manifest.json").exists());
        assert!(manifest.expected_fanout_multiplier >= 2.4);
        assert_eq!(
            (manifest.wave3_payment_mean_cents / manifest.wave1_payment_mean_usd).round(),
            100.0
        );
    }

    #[test]
    fn test_seed_is_strictly_deterministic() {
        let dir1 = tempdir().expect("dir1");
        let dir2 = tempdir().expect("dir2");

        let m1 = seed_validation_suite(42, 120, dir1.path()).expect("m1");
        let m2 = seed_validation_suite(42, 120, dir2.path()).expect("m2");

        assert_eq!(m1, m2);
    }
}
