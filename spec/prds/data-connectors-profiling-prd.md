# Universal Data Connectors, Schema Profiling & Autonomous Semantic Discovery PRD

## Overview
This PRD defines the requirements for connecting **Data-Engine** to heterogeneous data sources (S3/MinIO object storage, relational databases, and NoSQL document stores), inferring flat and nested schemas into **Apache Arrow**, computing statistical field profiles and **MinHash** value-overlap sketches, empirically probing candidate joins in a sandbox, and synchronizing discovered semantic metadata with **[`Graphrag`](https://github.com/PolecatWorks/Graphrag)** for human-in-the-loop verification.

---

## Core Objectives

1. **Universal Data Access (`connectors`)**: Read structured, semi-structured, and columnar datasets across S3/MinIO (Parquet, CSV, JSONL, Arrow IPC, Iceberg), relational databases (PostgreSQL, MySQL, SQLite, DuckDB), and NoSQL document collections (MongoDB, OpenSearch) through a unified Apache Arrow table abstraction.
2. **Automated Structure & Statistical Profiling (`profiler`)**: Automatically inspect schemas (including flattening nested NoSQL/JSON structures) and compute column-level statistical profiles (null rates, cardinality, quantiles, regex patterns, temporal grains, unit hints, and top-$k$ categorical values).
3. **Autonomous Semantic Layer Discovery**: Overcome human "Catalog Rot" by combining statistical MinHash value overlap, LLM semantic inference, and **Empirical Sandbox Probing** to automatically propose dataset grains, field meanings, and `JOINABLE_WITH` relationships.
4. **Epistemic Lifecycle & Human Stewardship**: Track every discovered field meaning and join relationship through an explicit epistemic state machine (`discovered` $\rightarrow$ `hypothesized` $\rightarrow$ `empirically_tested` $\rightarrow$ `pending_review` $\rightarrow$ `verified` / `rejected` $\rightarrow$ `drifted`) and synchronize verified/candidate knowledge into `Graphrag`.

---

## System Architecture & Discovery Flow

```mermaid
flowchart LR
    subgraph Sources["1. Registered Data Sources (/api/v1/sources)"]
        Obj["Object Store (S3 / MinIO / Local)\nParquet, CSV, JSONL, Arrow, Iceberg"]
        Rel["Relational DBs\nPostgreSQL, MySQL, DuckDB"]
        Doc["NoSQL / Document Stores\nMongoDB, OpenSearch"]
    end

    subgraph Profiler["2. Introspection & Profiling Engine (de-be-container)"]
        ArrowProj["Arrow Schema & Nested JSON Projector"]
        Stats["Streaming Column Profiler\n(Null %, Distinct, Min/Max, Quantiles, Samples)"]
        Sketch["MinHash & HyperLogLog Sketcher\n(Fast Join-Key Overlap Detection)"]
        Probe["Empirical Join Probe Runner\n(DataFusion Inclusion Ratio & 1:1 / 1:N / N:M Check)"]
    end

    subgraph Catalog["3. Data-Engine Catalog & GraphRAG Sync"]
        DB[("PostgreSQL (data_engine)\ndata_sources, datasets,\ndataset_fields, join_hypotheses")]
        GR["GraphRAG-RS (/api/v1/mcp)\nLayer 1 Dataset Nodes,\nLayer 0 Field Nodes & JOINABLE_WITH Tuples"]
    end

    subgraph Steward["4. Human-in-the-Loop Verification (de-fe-container)"]
        Canvas["RHS Shared Canvas MFE\n./DataAssetFormWrapper &\n./JoinHypothesisReviewWrapper"]
    end

    Obj & Rel & Doc --> ArrowProj
    ArrowProj --> Stats
    Stats --> Sketch
    Sketch -->|"Candidate Key Pairs\n(Jaccard >= threshold)"| Probe
    Probe --> DB
    DB <-->|"MCP Sync (node_add / edge_add)"| GR
    DB <-->|"REST & Live Canvas Patch"| Canvas
```

---

## Domain Model & Database Schema

```mermaid
erDiagram
    DATA_SOURCE ||--o{ DATASET : "provides"
    DATASET ||--o{ DATASET_FIELD : "contains"
    DATASET_FIELD ||--o{ JOIN_HYPOTHESIS : "left_field_id"
    DATASET_FIELD ||--o{ JOIN_HYPOTHESIS : "right_field_id"

    DATA_SOURCE {
        uuid id PK
        varchar name UK
        varchar kind "s3 | postgres | duckdb | mongodb | local_fs"
        jsonb connection_config "Endpoint, bucket, prefix, secret_ref"
        varchar status "connected | degraded | error"
        timestamptz last_checked_at
        timestamptz updated_at
    }

    DATASET {
        uuid id PK
        uuid source_id FK
        varchar canonical_id UK "snake_case identifier (shared with Graphrag)"
        varchar physical_locator "Table name, S3 URI, or collection name"
        varchar format "parquet | csv | jsonl | postgres_table | mongo_collection"
        varchar grain_description "e.g. One row per order line item"
        varchar epistemic_status "discovered | hypothesized | verified | drifted"
        bigint row_count_estimate
        jsonb schema_arrow_json "Serialized Arrow schema"
        varchar schema_fingerprint_sha256 "Detects structural schema drift"
        timestamptz last_profiled_at
        timestamptz updated_at
    }

    DATASET_FIELD {
        uuid id PK
        uuid dataset_id FK
        varchar canonical_id UK "dataset_id.field_name in snake_case"
        varchar field_path "Column name or JSON path (e.g. customer.address.zip)"
        varchar arrow_data_type "Int64, Utf8, Float64, Timestamp(Nanosecond, UTC)"
        boolean is_nullable
        boolean is_primary_key_candidate
        varchar semantic_label "AI/Human business concept label"
        text description "Factual and business description"
        varchar unit "e.g. USD, cents, seconds, UTC"
        varchar sensitivity_tag "public | internal | confidential | pii"
        varchar epistemic_status "discovered | hypothesized | empirically_tested | pending_review | verified | rejected | drifted"
        float8 confidence "0.0 to 1.0"
        jsonb profile_stats "null_ratio, distinct_count, min, max, quantiles, top_values, regex_pattern"
        bytea minhash_sketch "Serialized MinHash signature for value overlap"
        varchar verified_by "Nullable human steward identifier"
        timestamptz updated_at
    }

    JOIN_HYPOTHESIS {
        uuid id PK
        uuid left_field_id FK
        uuid right_field_id FK
        varchar predicate "JOINABLE_WITH | FORBIDDEN_JOIN | DERIVED_FROM"
        varchar cardinality "1:1 | 1:N | N:1 | N:M"
        float8 confidence "0.0 to 1.0"
        varchar epistemic_status "hypothesized | empirically_tested | pending_review | verified | rejected | drifted"
        text filter_invariant "Optional discovered WHERE clause (e.g. status != 'TEST')"
        jsonb empirical_proof "jaccard_overlap, left_inclusion_ratio, right_inclusion_ratio, orphan_samples, matched_samples"
        varchar verified_by "Nullable human steward identifier"
        text steward_notes "Human explanation or rejection rationale"
        timestamptz updated_at
    }
```

---

## Core Functional Requirements

### 1. Universal Data Source Connectors (`src/connectors/`)
- **Object Storage (`s3` / `local_fs`)**:
  - Uses `object_store` and `datafusion` to read `Parquet`, `CSV`, `JSONL`, and `Arrow IPC` files from S3-compatible buckets (AWS S3, MinIO) or local workspace directories.
  - Supports partitioned directory layouts (Hive-style `year=2026/month=11/*.parquet`) and predicate pushdown.
- **Relational Databases (`postgres` / `duckdb`)**:
  - Introspects `information_schema.columns` and foreign-key constraints, and registers tables as DataFusion `TableProvider`s for federated querying.
- **NoSQL & Semi-Structured Documents (`mongodb` / `jsonl`)**:
  - Samples documents to infer nested schemas, automatically projecting nested objects and scalar fields into flat dot-notation `field_path` descriptors (`customer.id`, `items[].sku`) backed by Arrow `StructArray` / `ListArray` representations so SQL and Python can query them uniformly.

---

### 2. Automated Statistical Profiling & Drift Detection (`src/profiler/`)
When `POST /api/v1/datasets/:id/profile` (or MCP tool `dataset_profile`) is invoked:
1. **Structural Fingerprinting**: Computes a deterministic SHA-256 fingerprint over sorted `(field_path, arrow_data_type, is_nullable)` tuples. If a previously `verified` dataset's fingerprint changes, affected fields and downstream `JOIN_HYPOTHESIS` records automatically transition to `epistemic_status = "drifted"`.
2. **Column Statistics**: Executes a single-pass DataFusion aggregation over the dataset (or a configurable sample window `profiler.max_sample_rows`) to compute:
   - `null_count` and `null_ratio`
   - Approximate/exact `distinct_count` and uniqueness ratio (`is_primary_key_candidate` when `null_ratio == 0.0` and `uniqueness >= 0.999`)
   - Numeric/temporal `min`, `max`, `mean`, and `p25` / `p50` / `p75` quantiles
   - Top-$k$ most frequent categorical values (`top_values`) so AI query planners know exact enum literals (e.g., `'ACTIVE'` vs `'ACT'`).
3. **MinHash Value Sketching**: For candidate identifier, string, and integer key columns, computes a 128-hash MinHash signature (`minhash_sketch`) to estimate Jaccard value-set similarity across columns in $O(1)$ time without full cross-table scans.

---

### 3. Empirical Join Probing & Fan-Out Trap Analysis
To prevent LLMs from hallucinating invalid joins or falling into **fan-out / chasm traps**, `data-engine` empirically validates candidate joins via `POST /api/v1/join-hypotheses/probe`:

```mermaid
sequenceDiagram
    autonumber
    participant Agent as Discovery Agent / Steward
    participant Profiler as Profiler & Probe Engine
    participant DF as Apache DataFusion
    participant DB as PostgreSQL (join_hypotheses)
    participant GR as GraphRAG-RS (/api/v1/mcp)

    Agent->>Profiler: POST /api/v1/join-hypotheses/probe { left_field_id, right_field_id }
    Profiler->>DF: Execute read-only probe query (distinct counts, intersection, orphans)
    DF-->>Profiler: left_inclusion: 0.992, right_unique: true, left_unique: false
    Profiler->>Profiler: Determine cardinality = "N:1" (Safe from right-side fan-out)
    Profiler->>Profiler: Extract 3 matched sample rows + 3 unmatched orphan values
    Profiler->>DB: Upsert JOIN_HYPOTHESIS (status = "empirically_tested", confidence = 0.94, empirical_proof)
    Profiler->>GR: Sync KnowledgeObjects & GraphTuple (JOINABLE_WITH) with metadata
    Profiler-->>Agent: 200 OK (JoinHypothesis with empirical_proof & cardinality)
```

- **Negative Constraints (`FORBIDDEN_JOIN`)**: When a human steward rejects a join hypothesis (`PUT /api/v1/join-hypotheses/:id` with `epistemic_status = "rejected"`), the record is preserved with `predicate = "FORBIDDEN_JOIN"` and `steward_notes`. Both `data-engine`'s AST validator and `Graphrag` block future queries from using that rejected join path.

---

### 4. Bidirectional Synchronization with `Graphrag`
When `graphrag.enabled = true` in `AppConfig`:
- Profiling or updating a `Dataset` upserts a Layer 1 `KnowledgeObject` (`label = "Dataset"`) in `Graphrag`.
- Profiling or curating a `DatasetField` upserts a Layer 0 `KnowledgeObject` (`label = "Field"`) and links `(Dataset) -[HAS_FIELD]-> (Field)`.
- Probing or verifying a `JoinHypothesis` upserts a `(LeftField) -[JOINABLE_WITH]-> (RightField)` (or `FORBIDDEN_JOIN`) tuple in `Graphrag` carrying the cardinality, confidence score, and epistemic status in its description/metadata.
