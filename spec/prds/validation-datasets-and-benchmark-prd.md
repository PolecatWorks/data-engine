# Validation Datasets, Benchmark Harness & Chaos Seeding PRD

## Overview
A core risk in building an **Autonomous Semantic Layer (`Graphrag`), Governed Steward Agents (`agent-as-data`), Sandboxed Compute & Provenance Engine (`data-engine`), and Bidirectional Shared Canvas (`Angry-agent` + `Mfe-shell`)** is validating that the system genuinely solves real-world data entropy rather than only working on clean toy demos.

This PRD defines:
1. **Curated External Validation Datasets & Benchmarks** (where to acquire them, how they are ingested, and what failure modes each tests).
2. **The Built-in Enterprise Chaos & Drift Seeder (`de-be seed-validation-suite`)**: A deterministic, zero-network-dependency data generator built into `data-engine` that materializes multi-source tables (Parquet on S3/MinIO, PostgreSQL, and JSONL) containing **5 canonical semantic & join traps** across **3 temporal drift waves**, paired with a cryptographic `ground_truth_manifest.json`.
3. **The Epistemic Ablation & Evaluation Harness**: Quantitative metrics proving how accuracy, join safety, and provenance reproducibility improve across each stage of the **Autonomous $\rightarrow$ Human-in-the-Loop (HITL) Verified** lifecycle.

---

## Validation Architecture & Three-Tier Dataset Strategy

```mermaid
flowchart LR
    subgraph Sources["1. Validation Data Sources"]
        S1["Tier 1: Built-in Chaos Seeder\n(de-be seed-validation-suite)\n• TPC-H via tpchgen-rs\n• 5 Planted Semantic Traps\n• 3 Temporal Drift Waves"]
        S2["Tier 2: Open-Source Multi-Table Datasets\n• Olist E-Commerce (9 tables, Kaggle/HF)\n• NYC TLC Taxi + Weather (AWS S3 Parquet)"]
        S3["Tier 3: Academic Ground-Truth Benchmarks\n• BIRD-SQL Mini-Dev (500 dirty DB queries)\n• Valentine Schema/Join Discovery Suite"]
    end

    subgraph Pipeline["2. Ecosystem Under Test"]
        DE["data-engine\nProfile + Sketch + Probe + Sandbox"]
        GR["Graphrag\n3-Layer Ontology & Epistemic States"]
        AAD["agent-as-data\nSemanticSteward & ProvenanceDataAnalyst"]
        AA["Angry-agent + Mfe-shell\nRHS Shared Canvas HITL Review"]
    end

    subgraph Eval["3. Quantitative Validation Gates"]
        M1["Join Discovery Precision & Recall\n(Target: >= 92% Recall, 0 Forbidden Joins)"]
        M2["Fan-Out Trap Interception Rate\n(Target: 100% caught pre-aggregation)"]
        M3["Ablation Accuracy Lift\n(Raw LLM vs. Verified Semantic Layer)"]
        M4["Provenance Replay Determinism\n(100% SHA-256 Digest Match)"]
    end

    Sources --> Pipeline --> Eval
```

---

## 1. Curated External Datasets & Where to Acquire Them

### 1.1 Tier 1: Academic Benchmarks with Ground-Truth Answers
| Dataset / Benchmark | Where to Acquire | Scale & Format | Specific Capabilities Validated |
| :--- | :--- | :--- | :--- |
| **BIRD-SQL (`bird_mini_dev`)** | • Website: `https://bird-bench.github.io/`<br/>• HuggingFace: `birdsql/bird_mini_dev`<br/>• Direct CLI: `de-be fetch-benchmark --suite bird-mini` | 11 enterprise domains (`financial`, `california_schools`, `formula_1`, `thrombosis_prediction`, `debit_card_specializing`, etc.); 500 complex SQL/Python questions with ground-truth SQL + domain `evidence`. | **Cryptic Column Grounding & Business Formula Stewardship**: Tests holding back BIRD's `evidence` rules initially, letting `data-engine` + `Graphrag` infer candidate mappings, and using HITL Canvas review to inject domain formulas. |
| **Valentine Dataset Discovery (`delftdata/valentine`)** | • GitHub: `https://github.com/delftdata/valentine`<br/>• Zenodo Benchmark Suites (TPC-DI, Wikidata, Magellan, ChEMBL splits) | Hundreds of table pairs with controlled vertical/horizontal splits, obfuscated column names, and value noise + exact ground-truth join mappings. | **Automated Join-Key & Synonym Discovery**: Directly benchmarks `data-engine`'s MinHash Jaccard sketcher + `Graphrag`'s BGE-M3 column embedding matcher (Precision@K, Recall@K). |
| **Spider 2.0 (`xlang-ai/Spider2`)** | • Website: `https://spider2-sql.github.io/`<br/>• GitHub: `https://github.com/xlang-ai/Spider2` | Enterprise-wide schemas (50–200+ columns, nested JSON + Parquet) requiring multi-step SQL + Python scripts. | **Multi-Hop Join Planning & Dynamic Python Sandbox Execution**. |

### 1.2 Tier 2: Real-World Multi-Table Open Data (Parquet / CSV / JSONL)
| Dataset | Where to Acquire | Scale & Format | Specific Traps & Capabilities Validated |
| :--- | :--- | :--- | :--- |
| **Brazilian E-Commerce by Olist** | • Kaggle: `olistbr/brazilian-ecommerce`<br/>• HuggingFace Parquet mirrors<br/>• Direct CLI: `de-be fetch-benchmark --suite olist` | 9 tables (`orders`, `order_items`, `order_payments`, `order_reviews`, `customers`, `sellers`, `products`, `geolocation`, `category_translation`), ~100k orders. | 1. **`customer_id` vs `customer_unique_id` Trap**: `orders.customer_id` is per-order; true repeat customer key is `customers.customer_unique_id`.<br/>2. **M:N Payment/Item Chasm Trap**: Joining `orders` + `order_items` + `order_payments` inflates GMV unless pre-aggregated.<br/>3. **Python Sandbox Analytics**: RFM segmentation, Haversine shipping distance vs. review score regression. |
| **NYC TLC Trip Records + NOAA Weather** | • AWS Open Data S3: `s3://nyc-tlc/trip data/`<br/>• HTTP Parquet: `https://www.nyc.gov/site/tlc/about/tlc-trip-record-data.page` | Multi-million row Parquet files (`yellow_tripdata_YYYY-MM.parquet`) + `taxi_zone_lookup.csv` + hourly weather JSONL. | **Zero-Copy S3 Parquet Pushdown, Cross-Format Federation (Parquet + CSV + JSONL), and Historical Schema Drift** across years. |
| **TPC-H (via `tpchgen-rs`)** | • Built directly into `de-be-container` via the pure-Rust `tpchgen` & `tpchgen-arrow` crates (zero network download). | 8 relational tables (`lineitem`, `orders`, `customer`, `part`, `supplier`, `partsupp`, `nation`, `region`) at configurable scale factor (`SF=0.01` for fast CI, `SF=1.0+` for perf). | **Deterministic CI Performance Benchmarking & Multi-Hop Join Path Planning (`graphrag_find_join_path`)**. |

---

## 2. Built-in Deterministic Chaos & Drift Seeder (`de-be seed-validation-suite`)

To guarantee that every PR and integration test run can validate the full semantic lifecycle without external network dependencies, `de-be-container` provides a CLI subcommand:

```bash
de-be seed-validation-suite \
  --output-dir /tmp/validation-lakehouse \
  --postgres-dsn postgresql://postgres:postgres@localhost:5432/validation_db \
  --wave all
```

### 2.1 The 5 Canonical Semantic & Join Traps Planted by the Seeder

```mermaid
erDiagram
    CRM_CUSTOMERS ||--o{ ERP_SALES_HDR : "TRAP-02: Cryptic Synonym Join (party_guid == c_pty_ref, 97% Jaccard)"
    CRM_ACCOUNTS }|--|{ ERP_GL_ENTRIES : "TRAP-01: Homonym Chasm Trap (account_id != account_id -> FORBIDDEN_JOIN)"
    ERP_SALES_HDR ||--|{ ERP_SALES_SHIPMENTS : "TRAP-03: 1:N Fan-Out Trap (2.4x shipments per order)"
    ERP_SALES_HDR ||--|{ ERP_SALES_LINES : "TRAP-03: 1:N Fan-Out Trap (3.8x lines per order)"
    ERP_SALES_HDR ||--|| ERP_SALES_HDR_WAVE3 : "TRAP-04: Wave-3 Schema & Unit Drift (net_amt_minor cents -> net_amt USD)"
    CRM_CUSTOMERS ||--o{ SUPPORT_TICKETS_JSONL : "TRAP-05: Cross-Format Sandbox Anomaly Detection"
```

1. **`TRAP-01` — Homonym False-Join (`FORBIDDEN_JOIN` Validation)**:
   - **Data**: `postgres.crm.accounts.account_id` (`INT64`, range `1000..1500`, representing Customer Account IDs) and `s3://lake/erp/gl_entries.parquet#account_id` (`INT64`, range `1000..1500`, representing General Ledger Chart-of-Account codes like `1010 = Accounts Receivable`).
   - **Pass Criteria**: Statistical profiler detects numeric overlap; Steward rejects the proposed edge in the RHS Canvas (`JoinHypothesisReviewWrapper`) with rationale *"GL Account Code != Customer Account ID"*; `Graphrag` creates a `FORBIDDEN_JOIN` edge; subsequent analyst queries are strictly blocked from joining the two tables on `account_id`.

2. **`TRAP-02` — Cryptic Synonym Key Discovery**:
   - **Data**: `postgres.crm.customers.party_guid` (`VARCHAR`) and `s3://lake/erp/sales_hdr.parquet#c_pty_ref` (`VARCHAR`). Zero lexical similarity in column names, but `0.97` MinHash Jaccard value overlap and verified `1:N` (`customers` $\rightarrow$ `sales_hdr`) key cardinality.
   - **Pass Criteria**: `data-engine` autonomously discovers the candidate join via MinHash sketching, runs an empirical `DataFusion` join probe (`97.4%` match rate, `0` fan-out), and surfaces the hypothesis card in `Angry-agent` for 1-click Steward verification.

3. **`TRAP-03` — Multi-Table `1:N` / `M:N` Fan-Out Revenue Inflation**:
   - **Data**: `sales_hdr` (`1,000` orders, true net revenue `\$250,000.00`) related to `sales_shipments` (`2,400` rows, multiple split shipments per order) and `sales_lines` (`3,800` rows).
   - **Pass Criteria**: When asked *"What is total order revenue broken down by shipping carrier?"*, a naive join between `sales_hdr` and `sales_shipments` inflates revenue to `\$612,400.00` ($2.45\times$). `data-engine`'s pre-execution cardinality guard detects that `sales_hdr -> sales_shipments` is `1:N` and forces pre-aggregation or grain-aware allocation so the total equals the exact ground truth `\$250,000.00`.

4. **`TRAP-04` — Wave-3 Temporal Schema & Unit Drift**:
   - **Data**:
     - **Wave 1 & 2**: `s3://lake/erp/sales_hdr_2026_q1.parquet` stores `net_amt_minor` (`INT64`, cents, mean `25000`).
     - **Wave 3**: `s3://lake/erp/sales_hdr_2026_q2.parquet` renames `net_amt_minor` $\rightarrow$ `net_amt` (`FLOAT64`, USD dollars, mean `250.00` — a $100\times$ distribution shift) and introduces a new status code `'PARTIAL_REFUND'`.
   - **Pass Criteria**: Re-profiling triggers `graphrag_flag_schema_drift`; the `Net Revenue` ontology edge transitions from `verified` $\rightarrow$ `drifted`; `agent-as-data` halts unverified executive aggregation and prompts the Steward on the RHS Shared Canvas to verify the updated unit conversion (`net_amt * 100` or `/ 100`).

5. **`TRAP-05` — Dynamic Python Sandbox Algorithm & Cryptographic Provenance**:
   - **Data**: `sales_hdr` (Parquet) + `support_tickets.jsonl` (JSONL logs with sentiment & resolution times) with 5 planted anomalous churn accounts (`CUST-0042`, `CUST-0108`, `CUST-0189`, `CUST-0251`, `CUST-0399`).
   - **Pass Criteria**: Analyst asks to *"Run a rolling z-score & survival churn algorithm combining order velocity and support ticket escalations"*. `data-engine` executes the federated join in `DataFusion`, transfers the Arrow `RecordBatch` via `/dev/shm` into the isolated Python sandbox, identifies all 5 ground-truth anomalous accounts, and records a `ProvenanceRun` whose `input_dataset_hashes + code_sha256 + output_arrow_schema_hash` is 100% reproducible on replay.

---

## 3. Ground-Truth Manifest & Automated Ablation Matrix

The seeder outputs `ground_truth_manifest.json` containing:
- Exact ground-truth join edges (`verified_joins` and `forbidden_joins`).
- 25 benchmark business questions with exact expected scalar/tabular answers (`expected_rows`, `expected_checksum`, `tolerance: 1e-6`).

### Required Evaluation Matrix (`make validate-concepts`)
Every milestone run compares 3 epistemic configurations against `ground_truth_manifest.json`:

| Configuration | Description | Expected Outcome on Chaos Suite |
| :--- | :--- | :--- |
| **1. Baseline (Raw LLM + Raw Schemas)** | LLM queries tables directly without `Graphrag` ontology or `data-engine` fan-out guards. | Fails `TRAP-01` (joins GL code to Customer ID), `TRAP-02` (misses `c_pty_ref`), `TRAP-03` ($2.45\times$ revenue inflation), and `TRAP-04` ($100\times$ unit error). (~20–30% accuracy). |
| **2. Autonomous Discovery Only (Pre-HITL)** | `data-engine` profiling + MinHash + pre-execution fan-out guard active; `Graphrag` edges in `empirically_tested` status. | Catches `TRAP-02` (synonym key) and `TRAP-03` (fan-out guard), flags `TRAP-01` and `TRAP-04` as low-confidence/ambiguous requiring Steward confirmation. (~75–85% accuracy, 0 silent corruptions). |
| **3. Full Autonomous + HITL Verified** | Human Steward reviews & approves/rejects the flagged cards in the RHS Shared Canvas (`verified` + `FORBIDDEN_JOIN`). | **100% accuracy across all 5 traps**, 0% fan-out inflation, and 100% deterministic W3C PROV / OpenLineage provenance replay. |

---

## Related PRDs
- [Ecosystem Architecture & End-to-End Journeys PRD](./ecosystem-architecture-and-journeys-prd.md)
- [Data Connectors, Ingestion & Statistical Profiling PRD](./data-connectors-profiling-prd.md)
- [Federated Compute, Code Sandbox & Provenance Engine PRD](./compute-sandbox-provenance-prd.md)
- [GraphRAG Autonomous Semantic Ontology PRD](https://github.com/PolecatWorks/Graphrag/blob/feat/autonomous-semantic-data-canvas-prds/spec/prds/autonomous-semantic-ontology-prd.md)
- [Agent-as-Data Semantic Stewardship & Provenance PRD](https://github.com/PolecatWorks/agent-as-data/blob/feat/autonomous-semantic-data-canvas-prds/spec/prds/data-semantic-stewardship-and-provenance-prd.md)
