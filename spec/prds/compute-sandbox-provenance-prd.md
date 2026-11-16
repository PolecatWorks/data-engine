# Federated Compute, Sandboxed Python Execution & Cryptographic Provenance PRD

## Overview
This PRD defines the requirements and architecture for **Data-Engine's** analytical calculation and dynamic code execution subsystem:
1. **Federated SQL & DataFrame Calculation Engine (`src/compute/`)**: Powered by **Apache DataFusion**, `sqlparser-rs`, and **Apache Arrow**, enabling cross-source joins and aggregations with deterministic AST validation, fan-out/chasm trap detection, and column-level lineage extraction.
2. **Rust-Supervised Dynamic Python Sandbox (`src/sandbox/`)**: A lightweight, isolated code execution environment managed by Rust that stages DataFusion outputs into **zero-copy Apache Arrow shared memory (`/dev/shm` / `memfd`)** and executes dynamic Python algorithms inside a namespace/seccomp jail (`NamespaceSandbox`) or isolated worker (`SidecarSandbox`).
3. **Immutable Provenance & Decision Lineage Ledger (`src/provenance/`)**: A W3C PROV and OpenLineage-compatible audit trail that records the complete chain of reasoning, dataset versions, verified vs. hypothesized business rules, SQL AST / Python SHA-256 digests, and output hashes for every run.

---

## End-to-End Execution & Provenance Architecture

```mermaid
flowchart TB
    subgraph Request["1. Query or Algorithm Request"]
        Q["Federated SQL Query\nOR Dynamic Python Algorithm"]
        Rules["Referenced Semantic Rules & Join Hypotheses\n(from Graphrag / data-engine catalog)"]
    end

    subgraph Validator["2. Deterministic Gatekeeper (src/compute/)"]
        SQLParser["sqlparser-rs AST Inspector\n• Block DDL/DML mutations\n• Extract referenced tables, columns & join keys"]
        GrainCheck["Grain & Fan-Out Validator\n• Reject FORBIDDEN_JOIN edges\n• Detect un-aggregated 1:N * 1:N chasm traps\n• Enforce sensitivity / PII guardrails"]
        DFPlan["Apache DataFusion LogicalPlan\n• Type & schema verification across S3/Postgres/NoSQL"]
    end

    subgraph Execution["3. Zero-Copy Compute & Sandbox (src/sandbox/)"]
        DFExec["DataFusion Physical Execution\n(Streams S3 Parquet + DB tables -> Arrow RecordBatches)"]
        ArrowShm[("Read-Only Arrow IPC Buffer\n(/dev/shm/de_run_<uuid>.arrow)")]
        Supervisor["Rust Sandbox Supervisor\n(Linux Namespaces / Seccomp / Rlimits / Timeout)"]
        PyWorker["Isolated Python Worker\n(Zero-copy pyarrow / polars / numpy / scipy / sklearn)"]
        ArrowOut[("Output Arrow IPC Buffer\n(/dev/shm/de_out_<uuid>.arrow)")]
    end

    subgraph Provenance["4. Cryptographic Provenance Ledger (src/provenance/)"]
        Hasher["SHA-256 Lineage Signer\nH(Input Snapshots || Rules || AST/Code || Output)"]
        ProvDB[("PostgreSQL Provenance Tables\nprovenance_runs, provenance_lineage_edges")]
        MermaidGen["Mermaid Lineage DAG Generator"]
    end

    Q & Rules --> SQLParser
    SQLParser --> GrainCheck
    GrainCheck --> DFPlan
    DFPlan --> DFExec
    DFExec -->|"SQL-Only Query"| Hasher
    DFExec -->|"Input to Python Algorithm"| ArrowShm
    ArrowShm -->|"mmap (<1ms)"| PyWorker
    Supervisor -->|"Spawns & Enforces Quotas"| PyWorker
    PyWorker --> ArrowOut
    ArrowOut --> Hasher
    Hasher --> ProvDB
    ProvDB --> MermaidGen
```

---

## Core Functional Requirements

### 1. Deterministic SQL AST & Grain Validation (`src/compute/`)
Before any SQL query or data staging query is executed against underlying data sources:
1. **Read-Only AST Enforcement (`sqlparser-rs`)**:
   - Only `SELECT` (including CTEs `WITH ... AS`, window functions, and set operations) is permitted. Any `INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `COPY`, or administrative statement is rejected immediately with `ComputeError::ForbiddenStatementType`.
2. **Catalog & Join-Hypothesis Verification**:
   - Extracts all `(left_table.left_col = right_table.right_col)` join predicates from the AST.
   - Verifies that none of the join pairs match a `FORBIDDEN_JOIN` (`epistemic_status = "rejected"`) in `join_hypotheses` (returning `ComputeError::ForbiddenJoinPath` with the human steward's rejection notes).
   - **Chasm / Fan-Out Trap Detection**: If a query joins table $A$ to two independent $1:N$ child tables ($A \rightarrow B$ with `1:N` and $A \rightarrow C$ with `1:N`) in the same query block without pre-aggregating $B$ and $C$ in CTEs/subqueries, the validator flags `ComputeError::FanOutTrapDetected` and suggests the pre-aggregated CTE structure.
3. **Bounded Resource Quotas (`ComputeConfig`)**:
   - Enforces `max_output_rows` (e.g., `10_000`), `memory_limit_bytes` via DataFusion's `FairSpillPool`, and `query_timeout_seconds`.

---

### 2. Rust-Managed Dynamic Python Sandbox (`src/sandbox/`)

#### A. Versioned Algorithm Registry (`algorithms` & `algorithm_revisions`)
Dynamic code is never executed as an anonymous, untracked string. Every algorithm is persisted as a versioned data artifact:
- `id` (UUID), `canonical_id` (`snake_case`), `name`, `description`
- `language`: `"python"` or `"sql"`
- `code`: Source code string
- `code_sha256`: Deterministic SHA-256 hash of normalized source code
- `input_ bindings`: Named map of input dataset queries or dataset IDs (mounted as read-only Arrow tables `inputs["orders"]`, `inputs["customers"]`)
- `expected_output_schema`: Optional JSON Arrow schema contract
- `validation_status`: `"draft" | "validated" | "failed"`
- `current_version`: Monotonically incrementing revision number persisted in `algorithm_revisions`

#### B. Zero-Copy Apache Arrow Data Bridge
1. For each entry in `input_bindings`, Rust executes the validated DataFusion plan and writes the resulting `RecordBatch` stream into an ephemeral, read-only Apache Arrow IPC file in shared memory (`/dev/shm/de_run_<run_id>/in_<name>.arrow`).
2. Inside the sandbox, a lightweight Python bootstrap harness memory-maps each input via `pyarrow.ipc.open_file` (or `polars.read_ipc(..., memory_map=True)`) in under `1ms` with zero heap copy.
3. The user/agent's Python function `def run(inputs: dict[str, pl.DataFrame | pa.Table], params: dict) -> pl.DataFrame | pa.Table | dict:` executes and writes its output Arrow table to `/dev/shm/de_run_<run_id>/out_result.arrow` plus structured summary metrics to `out_metrics.json`.
4. Rust validates the output Arrow schema, reads the result, and immediately unlinks the `/dev/shm/de_run_<run_id>` directory in a `Drop` guard.

#### C. Pluggable Sandbox Isolation Backends (`SandboxBackend` Trait)
`SandboxConfig` (`src/sandbox/mod.rs`) supports configurable isolation backends behind a unified `SandboxExecutor` trait:

```mermaid
classDiagram
    class SandboxExecutor {
        <<trait>>
        +validate_code(code: &str) Result~ValidationReport, SandboxError~
        +execute(job: SandboxJob) Result~SandboxOutput, SandboxError~
    }
    class NamespaceSandbox {
        +Linux User/Mount/PID/Net Namespaces (bwrap / hakoniwa)
        +CLONE_NEWNET (Zero Network Egress)
        +Read-only Python venv + /dev/shm Arrow mount
        +RLIMIT_AS & RLIMIT_CPU + Tokio timeout
    }
    class ProcessSandbox {
        +Restricted Subprocess w/ AST Gatekeeper
        +Environment scrubbing (env_clear)
        +Ephemeral workdir & RLIMIT / Tokio timeout
        +Fallback for restricted container environments
    }

    SandboxExecutor <|.. NamespaceSandbox
    SandboxExecutor <|.. ProcessSandbox
```

1. **Static AST Pre-Flight Gatekeeper**: Before spawning any process, the sandbox validates the Python AST to reject forbidden imports/builtins (`socket`, `subprocess`, `os.system`, `pty`, `urllib`, `requests`, `http`, `ctypes`, `open` outside the sandbox output path).
2. **`namespace` Mode (Default on Linux hosts)**: Uses Linux namespaces (`CLONE_NEWNET` for zero network access, `CLONE_NEWNS` for read-only rootfs and Python virtualenv bind-mount, `CLONE_NEWPID`) and memory/CPU resource limits.
3. **`process` Mode (Container-safe fallback)**: Uses scrubbed environment (`Command::env_clear()`), read-only input permissions, AST restriction, and strict wall-clock/memory limits when running inside locked-down Kubernetes pods without nested `CLONE_NEWUSER` privileges.

---

### 3. Cryptographic Provenance & Decision Lineage Ledger (`src/provenance/`)

Every query execution (`POST /api/v1/queries/execute`) and algorithm execution (`POST /api/v1/algorithms/:id/execute`) creates an immutable **`ProvenanceRun`** record in PostgreSQL:

```mermaid
erDiagram
    PROVENANCE_RUN ||--o{ PROVENANCE_LINEAGE_EDGE : "records"
    ALGORITHM ||--o{ ALGORITHM_REVISION : "versions"
    ALGORITHM_REVISION ||--o{ PROVENANCE_RUN : "executes"

    PROVENANCE_RUN {
        uuid id PK
        varchar run_type "sql_query | python_algorithm | join_probe"
        text question_or_intent "Original natural language question or objective"
        uuid algorithm_id FK "Nullable algorithm ID"
        int algorithm_version "Nullable version number"
        text executed_sql_or_code "Exact SQL or Python source executed"
        varchar code_or_ast_sha256 "SHA-256 digest of normalized AST/code"
        jsonb input_dataset_snapshots "Array of {dataset_id, schema_sha256, row_count}"
        jsonb semantic_rules_used "Array of {rule_id, kind, epistemic_status, confidence, verified_by}"
        boolean all_rules_human_verified "True only if every join/rule used is 'verified'"
        float8 min_rule_confidence "Lowest confidence score among used rules"
        bigint output_row_count
        jsonb output_schema_arrow "Arrow schema of output"
        varchar output_digest_sha256 "SHA-256 digest of output data batch"
        varchar provenance_signature_sha256 "Master SHA-256(inputs || rules || code || output)"
        bigint execution_duration_ms
        varchar caller_identity "Agent ID or User ID"
        timestamptz created_at
    }

    PROVENANCE_LINEAGE_EDGE {
        uuid id PK
        uuid run_id FK
        varchar source_node_id "Dataset, Field, SemanticRule, or Algorithm ID"
        varchar source_node_type "dataset | field | join_hypothesis | business_rule | algorithm"
        varchar predicate "READ_FROM | JOINED_VIA | GOVERNED_BY | TRANSFORMED_BY | PRODUCED"
        varchar target_node_id "Target Field, Algorithm, or Output Column"
        varchar epistemic_status "verified | empirically_tested | hypothesized"
    }
```

#### Key Provenance Capabilities:
1. **Epistemic Trust Rollup (`all_rules_human_verified` & `min_rule_confidence`)**:
   - Every execution response includes a top-level trust summary telling the user and calling agent whether the result relied exclusively on **Human-Verified (`verified`)** semantic rules or included **AI-Inferred (`empirically_tested` / `hypothesized`)** rules that still need human sign-off.
2. **Deterministic Mermaid Lineage DAG (`GET /api/v1/provenance/:id/mermaid`)**:
   - Automatically renders a complete Mermaid flowchart showing source datasets $\rightarrow$ fields $\rightarrow$ join hypotheses (color-coded by `verified` vs. `hypothesized`) $\rightarrow$ SQL/Python transformation $\rightarrow$ output columns, ready to mount directly in `Angry-agent` via `MermaidShowWrapper` or `./ProvenanceLineageWrapper`.
