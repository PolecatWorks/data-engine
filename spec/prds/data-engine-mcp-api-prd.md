# Data-Engine REST BREAD & Model Context Protocol (MCP) Service PRD

## Overview
This PRD defines the REST API and **Model Context Protocol (MCP)** service contracts for **Data-Engine (`de-be-container`)**, following the established patterns of [`Graphrag`](https://github.com/PolecatWorks/Graphrag) and [`agent-as-data`](https://github.com/PolecatWorks/agent-as-data).

All REST and MCP endpoints are mounted under the configurable `service.api_prefix` (defaulting in `config/default.yaml` and `charts/data-engine/configs/config.yaml` to `"/api/data-engine/v1"`, overridable via `DE_BE__SERVICE__API_PREFIX`) so Kubernetes Istio `VirtualService` ingress can route `/api/data-engine/v1/*` directly to `de-be-container` without URI path rewrites. For backwards compatibility with existing internal probes and integration tests, when `service.api_prefix != "/api/v1"`, the Axum router also mounts the routes under `/api/v1/*` and exposes `/health` at the root.

By exposing all Data-Engine capabilities over both REST (`/api/data-engine/v1/*` and `/api/v1/*`) and stateless HTTP JSON-RPC 2.0 MCP (`/mcp`, `/api/data-engine/v1/mcp`, and `/api/v1/mcp`), autonomous agents in **`agent-as-data`** and **`Angry-agent`** as well as human stewards using **`de-fe-container`** can:
1. Verify connectivity via the `/health` endpoint (`/api/data-engine/v1/health`) and canonical **`hello`** MCP tool.
2. Register and browse external data sources (`source_*`).
3. Introspect, profile, and curate datasets and fields (`dataset_*`, `field_*`).
4. Discover, empirically probe, and verify or reject join hypotheses (`join_*`).
5. Validate and execute federated SQL queries (`query_*`) and sandboxed Python algorithms (`algorithm_*`).
6. Inspect end-to-end cryptographic provenance and Mermaid lineage graphs (`provenance_*`).

---

## REST BREAD & Pipeline API Specification (`:8080`, mounted at `${service.api_prefix}` e.g. `/api/data-engine/v1` and `/api/v1`)

### 1. Data Sources Resource (`/api/v1/sources`)

| BREAD Op | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Browse** | `GET /api/v1/sources` | `200 OK` | Lists registered data sources (`s3`, `postgres`, `duckdb`, `mongodb`, `local_fs`) with status and dataset counts. |
| **Read** | `GET /api/v1/sources/{id}` | `200 OK` | Retrieves a single `DataSource` record and its discovered datasets. Returns `404 Not Found` if absent. |
| **Edit** | `PUT /api/v1/sources/{id}` | `200 OK` | Updates connection configuration and re-verifies source connectivity. |
| **Add** | `POST /api/v1/sources` | `201 Created` | Registers a new data source, validates connectivity, and optionally triggers initial dataset discovery. |
| **Delete** | `DELETE /api/v1/sources/{id}` | `204 No Content` | Deletes a data source and cascades removal to unreferenced child datasets. |

---

### 2. Datasets & Schema Introspection Resource (`/api/v1/datasets`)

| BREAD Op | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Browse** | `GET /api/v1/datasets` | `200 OK` | Lists datasets with optional filtering (`?source_id=&epistemic_status=&q=&limit=50&offset=0`). |
| **Read** | `GET /api/v1/datasets/{id}` | `200 OK` | Returns dataset metadata, Arrow schema, grain description, and all child `DatasetField` records. |
| **Edit** | `PUT /api/v1/datasets/{id}` | `200 OK` | Updates dataset `grain_description`, `epistemic_status`, or metadata, syncing updates to `Graphrag`. |
| **Add** | `POST /api/v1/datasets` | `201 Created` | Registers a dataset (`physical_locator`, `format`), introspects its Arrow schema, and creates initial `DatasetField` records. |
| **Profile** | `POST /api/v1/datasets/{id}/profile` | `200 OK` | Executes statistical column profiling, MinHash sketching, schema drift check, and candidate join discovery. |
| **Preview** | `GET /api/v1/datasets/{id}/preview` | `200 OK` | Returns up to `N` (default `25`, max `100`) sample rows as JSON records. |
| **Delete** | `DELETE /api/v1/datasets/{id}` | `204 No Content` | Deletes the dataset, its fields, and incident join hypotheses. |

---

### 3. Dataset Fields Resource (`/api/v1/fields`)

| BREAD Op | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Browse** | `GET /api/v1/fields` | `200 OK` | Lists fields across datasets with filtering (`?dataset_id=&epistemic_status=&sensitivity_tag=&q=`). |
| **Read** | `GET /api/v1/fields/{id}` | `200 OK` | Returns full field metadata, `profile_stats`, and incident `join_hypotheses`. |
| **Edit** | `PUT /api/v1/fields/{id}` | `200 OK` | Updates `semantic_label`, `description`, `unit`, `sensitivity_tag`, `epistemic_status`, and `verified_by`, syncing to `Graphrag`. |

---

### 4. Join Hypotheses & Empirical Probe Resource (`/api/v1/join-hypotheses`)

| BREAD Op | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Browse** | `GET /api/v1/join-hypotheses` | `200 OK` | Lists candidate, tested, verified, and rejected join relationships (`?dataset_id=&epistemic_status=`). |
| **Read** | `GET /api/v1/join-hypotheses/{id}` | `200 OK` | Returns full join hypothesis including `empirical_proof` (overlap ratios, cardinality, sample matched/orphan rows). |
| **Edit / Verify** | `PUT /api/v1/join-hypotheses/{id}` | `200 OK` | Human steward or agent updates `cardinality`, `filter_invariant`, `epistemic_status` (`verified` or `rejected` -> `FORBIDDEN_JOIN`), and `steward_notes`. |
| **Add / Probe** | `POST /api/v1/join-hypotheses/probe` | `201 Created` | Runs an empirical DataFusion overlap & cardinality probe between `left_field_id` and `right_field_id` and persists the resulting `JoinHypothesis`. |
| **Delete** | `DELETE /api/v1/join-hypotheses/{id}` | `204 No Content` | Removes a join hypothesis. |

---

### 5. Federated Queries Resource (`/api/v1/queries`)

| Operation | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Validate** | `POST /api/v1/queries/validate` | `200 OK` | Parses SQL via `sqlparser-rs` + DataFusion `LogicalPlan`, checks for `FORBIDDEN_JOIN`s and `1:N * 1:N` fan-out traps, extracts column lineage, and returns `{ valid, referenced_fields, join_hypotheses_used, warnings, errors }`. |
| **Execute** | `POST /api/v1/queries/execute` | `200 OK` | Validates and executes a federated SQL query across registered sources, persists a `ProvenanceRun` record, and returns `{ rows, schema, provenance_run_id, trust_summary }`. |

---

### 6. Sandboxed Algorithms Resource (`/api/v1/algorithms`)

| BREAD Op | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Browse** | `GET /api/v1/algorithms` | `200 OK` | Lists registered Python/SQL algorithms with current version and validation status. |
| **Read** | `GET /api/v1/algorithms/{id}` | `200 OK` | Returns the algorithm definition, `input_bindings`, code, and revision history (`algorithm_revisions`). |
| **Edit** | `PUT /api/v1/algorithms/{id}` | `200 OK` | Updates algorithm code/bindings, increments `current_version`, and creates an immutable `algorithm_revisions` snapshot. |
| **Add** | `POST /api/v1/algorithms` | `201 Created` | Creates a new versioned algorithm and runs static AST validation. |
| **Validate (Dry-Run)** | `POST /api/v1/algorithms/{id}/validate` | `200 OK` | Runs static AST checks and executes the algorithm in the sandbox against a bounded sample (`100` rows) of its `input_bindings` to verify output schema. |
| **Execute** | `POST /api/v1/algorithms/{id}/execute` | `200 OK` | Stages input datasets into zero-copy Arrow `/dev/shm`, runs the Python sandbox, records a `ProvenanceRun`, and returns `{ output_rows, metrics, provenance_run_id, trust_summary }`. |
| **Delete** | `DELETE /api/v1/algorithms/{id}` | `204 No Content` | Soft-archives or deletes an algorithm. |

---

### 7. Provenance & Lineage Resource (`/api/v1/provenance`)

| Operation | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Browse** | `GET /api/v1/provenance` | `200 OK` | Lists `ProvenanceRun` records with filtering by `run_type`, `algorithm_id`, or `all_rules_human_verified`. |
| **Read** | `GET /api/v1/provenance/{id}` | `200 OK` | Returns the full `ProvenanceRun` and all `ProvenanceLineageEdge` edges. |
| **Mermaid DAG** | `GET /api/v1/provenance/{id}/mermaid` | `200 OK` | Returns `{ mermaid: "flowchart LR...", graph_json: { nodes, edges } }` visualizing the complete decision and data lineage DAG. |

---

### 8. Curated Statistical Analytics Resource (`/api/v1/analytics`)
Detailed statistical contracts, `StatisticalResultEnvelope` schemas, and fail-fast Arrow IPC sampling rules are defined in [08. Statistical Analytics Engine PRD](./statistical-analytics-engine-prd.md).

| Operation | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Describe & Check** | `POST /api/v1/analytics/describe` | `200 OK` | Computes distributional moments, quantiles, normality tests, outlier flags, missingness diagnostics, and plot specs. |
| **Hypothesis Test** | `POST /api/v1/analytics/hypothesis-test` | `200 OK` | Executes parametric/non-parametric tests with automatic assumption checks, effect sizes, CIs, FDR/Bonferroni corrections, and optional stratified confounding checks. |
| **Power Analysis** | `POST /api/v1/analytics/power` | `200 OK` | Computes prospective/retrospective power, required sample size per arm, or Minimum Detectable Effect (MDE). |
| **Correlation & VIF** | `POST /api/v1/analytics/correlation` | `200 OK` | Computes Pearson/Spearman/Kendall matrices, partial correlations, FDR-adjusted $p$-values, and VIF multicollinearity diagnostics. |
| **Regression Fit** | `POST /api/v1/analytics/regression` | `200 OK` | Fits OLS, GLM (Gaussian/Binomial/Poisson/Gamma), or Huber/RANSAC robust regression with HC3 SEs, Breusch-Pagan, Durbin-Watson, and Cook's distance diagnostics. |
| **Time-Series Analyze** | `POST /api/v1/analytics/timeseries` | `200 OK` | Runs STL decomposition, ADF/KPSS stationarity tests, ACF/PACF, changepoint/anomaly detection, and ETS/ARIMA forecasting with prediction intervals. |

---

### 9. Data Visuals & Chart Explorer Resource (`/api/v1/visuals`)
Detailed `DataVisualSpec` schemas, schema-driven chart recommendations, and fail-fast `max_chart_points` rules are defined in [09. Data Visualizations & Semantic Relationship Topology PRD](./data-visuals-and-semantic-topology-prd.md).

| Operation | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Recommend** | `POST /api/v1/visuals/recommend` | `200 OK` | Deterministically ranks recommended chart specs from a `dataset_id` or validated SQL output schema and `DatasetField.profile_stats`. |
| **Query & Hydrate** | `POST /api/v1/visuals/query` | `200 OK` | Validates SQL/binding via `sqlparser-rs` + DataFusion grain checks, enforces `visuals.max_chart_points` (or explicit seeded sampling), records a `ProvenanceRun`, and returns `{ visual_spec, data_values, provenance }`. |
| **Browse** | `GET /api/v1/visuals` | `200 OK` | Lists saved `DataVisualSpec` artifacts (`?dataset_id=&chart_family=&q=`). |
| **Read** | `GET /api/v1/visuals/{id}` | `200 OK` | Retrieves a saved `DataVisualSpec` artifact by ID. |
| **Add** | `POST /api/v1/visuals` | `201 Created` | Persists a new named `DataVisualSpec` artifact. |
| **Edit** | `PUT /api/v1/visuals/{id}` | `200 OK` | Updates a saved `DataVisualSpec` specification, overlays, or metadata. |
| **Delete** | `DELETE /api/v1/visuals/{id}` | `204 No Content` | Deletes a saved visual artifact. |

---

### 10. Semantic & Structural Topology Resource (`/api/v1/topology`)
Detailed 3-layer ontology fusion, ER/join encoding, and Field-to-Field Anatomy DTOs are defined in [09. Data Visualizations & Semantic Relationship Topology PRD](./data-visuals-and-semantic-topology-prd.md).

| Operation | HTTP Method & Route | Success Status | Description |
| :--- | :--- | :--- | :--- |
| **Topology Graph** | `GET /api/v1/topology/graph` | `200 OK` | Returns a bounded, fused multi-layer `SemanticTopologyGraph` combining `data-engine` sources, datasets, fields, `JOINABLE_WITH` / `FORBIDDEN_JOIN` edges, and `Graphrag` Layer 2 Concepts, Metrics, Communities, and Confounders (`?lens=&source_id=&dataset_ids=&epistemic_status=&focus_node_id=&hops=2&include_fields=false&include_semantics=true`). |
| **Dataset Anatomy** | `GET /api/v1/topology/anatomy/dataset/{id}` | `200 OK` | Returns nested Arrow schema hierarchy tree, field profile mini-distributions, sensitivity/unit badges, and incident semantic concepts and joins. |
| **Join Anatomy** | `GET /api/v1/topology/anatomy/join/{id}` | `200 OK` | Returns side-by-side schema cards, key type compatibility, Venn/Sankey value-overlap breakdown, matched vs. orphan sample keys, and fan-out multiplier diagnostics. |
| **Mermaid Export** | `GET /api/v1/topology/mermaid` | `200 OK` | Exports any filtered topology view as deterministic Mermaid (`erDiagram` or `flowchart TB`) plus `graph_json`. |

---

## Model Context Protocol (MCP) Service Specification (`POST /mcp` & `POST /api/v1/mcp`)

Using `rmcp` (`#[tool_router]`, `#[tool]`) and `schemars::JsonSchema`, `DataEngineMcpServer` exposes the following structured tools over stateless HTTP JSON-RPC 2.0:

| Category | Tool Name | Required Parameters | Description |
| :--- | :--- | :--- | :--- |
| **Smoke Test** | `hello` | `name: String` | Canonical greeting verification tool returning `"Hello, <name>!"` (rejecting empty names with `isError: true`). |
| **Sources** | `source_add` | `name: String`, `kind: String`, `connection_config: Value` | Registers and verifies a new data source. |
| | `source_browse` | — | Lists registered data sources. |
| **Datasets & Profiling** | `dataset_add` | `source_id: String`, `physical_locator: String`, `format: String` | Registers a dataset and introspects its Arrow schema. |
| | `dataset_browse` | — | Lists datasets with optional `epistemic_status` or `q` filter. |
| | `dataset_read` | `id: String` | Retrieves dataset details, schema, and fields. |
| | `dataset_profile` | `id: String` | Runs statistical profiling, MinHash sketching, and `Graphrag` sync. |
| | `dataset_preview` | `id: String` | Returns sample rows from the dataset. |
| **Fields** | `field_browse` | — | Searches fields across datasets (`dataset_id`, `q`, `epistemic_status`). |
| | `field_edit` | `id: String` | Updates field `semantic_label`, `description`, `unit`, `sensitivity_tag`, or `epistemic_status`. |
| **Joins & Semantic Discovery** | `join_probe` | `left_field_id: String`, `right_field_id: String` | Empirically tests a candidate join in DataFusion and returns overlap ratios, cardinality (`1:1`, `1:N`), and sample rows. |
| | `join_browse` | — | Lists join hypotheses and verified/forbidden joins. |
| | `join_verify` | `id: String`, `epistemic_status: String` | Marks a join hypothesis as `verified` or `rejected` (`FORBIDDEN_JOIN`) with `steward_notes`. |
| **Federated Querying** | `query_validate` | `sql: String` | Validates SQL AST, checks grain/fan-out traps, and lists required join hypotheses. |
| | `query_execute` | `sql: String`, `question: String` | Executes a validated federated SQL query and returns results + `provenance_run_id` + `trust_summary`. |
| **Sandboxed Algorithms** | `algorithm_add` | `name: String`, `code: String`, `input_bindings: Value` | Creates a versioned Python/SQL algorithm and validates its AST. |
| | `algorithm_validate` | `id: String` | Dry-runs the algorithm in the sandbox on a 100-row sample. |
| | `algorithm_execute` | `id: String`, `question: String` | Executes the algorithm in the sandbox over zero-copy Arrow inputs and records full provenance. |
| | `algorithm_browse` | — | Lists registered algorithms. |
| **Statistical Analytics** | `stat_describe_and_check` | `input_binding: Value`, `columns: Vec<String>` | Runs distributional, normality, outlier, and missingness diagnostics returning a `StatisticalResultEnvelope`. |
| | `stat_hypothesis_test` | `input_binding: Value`, `method: String`, `value_col: String` | Runs parametric/non-parametric tests with automatic assumption checks, fallback results, CIs, effect sizes, and optional FDR/stratification. |
| | `stat_power_analysis` | `test_type: String`, `alpha: f64` | Computes sample size, power, or Minimum Detectable Effect (MDE). |
| | `stat_correlation_vif` | `input_binding: Value`, `columns: Vec<String>` | Computes Pearson/Spearman/Kendall matrices, partial correlations, and VIF multicollinearity diagnostics. |
| | `stat_regression_fit` | `input_binding: Value`, `model_type: String`, `target_col: String`, `feature_cols: Vec<String>` | Fits OLS/GLM/logistic/robust regression with HC3 SEs, Breusch-Pagan, Durbin-Watson, and Cook's distance diagnostics. |
| | `stat_timeseries_analyze` | `input_binding: Value`, `time_col: String`, `value_col: String` | Runs STL decomposition, ADF/KPSS stationarity tests, ACF/PACF, changepoints, and ETS/ARIMA forecasting. |
| **Data Visuals** | `visual_recommend` | `dataset_id_or_sql: Value` | Deterministically recommends ranked `DataVisualSpec` candidates (Vega-Lite/ECharts) using schema & field profile stats. |
| | `visual_render_spec` | `data_binding: Value`, `spec: Value` | Validates query/binding, executes bounded aggregation/sampling with provenance, and returns a canvas-ready `DataVisualSpec` payload for `./DataVisualExplorerWrapper`. |
| **Semantic & Structural Topology** | `topology_graph` | — | Returns a scoped multi-layer `SemanticTopologyGraph` (Business Concepts/Metrics $\rightarrow$ Datasets $\rightarrow$ Fields, Joins, `FORBIDDEN_JOIN`s, Confounders) for `./SemanticTopologyWrapper`. |
| | `topology_field_anatomy` | `target_kind: String`, `target_id: String` | Returns zoomed-in dataset nested-schema anatomy or field-to-field join overlap/fan-out anatomy for `./SemanticTopologyWrapper`. |
| **Provenance** | `provenance_read` | `id: String` | Returns the full cryptographic provenance bundle and Mermaid lineage diagram for a run. |
| | `provenance_browse` | — | Lists recent provenance runs. |
