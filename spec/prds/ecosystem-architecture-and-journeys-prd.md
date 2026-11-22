# 5-Repository Ecosystem Architecture & End-to-End Journeys PRD

## Overview
This PRD defines how **Data-Engine (`data-engine`)** integrates with the other four repositories in the PolecatWorks AI ecosystem on the shared feature track (`feat/autonomous-semantic-data-canvas-prds`):
1. **[`Mfe-shell`](https://github.com/PolecatWorks/Mfe-shell)**: Portal Host & `@polecatworks/mfe-shared` (`SharedCanvasService` + `MfeCanvasController`).
2. **[`Angry-agent`](https://github.com/PolecatWorks/Angry-agent)**: Conversational AI & RHS Live Shared Canvas Host (`MfeRenderer` + `MultiServerMCPClient`).
3. **[`agent-as-data`](https://github.com/PolecatWorks/agent-as-data)**: Declarative Agent Control Plane, `SemanticSteward` / `ProvenanceDataAnalyst` Traits, Epistemic Guardrails & Skill Registry.
4. **[`Graphrag`](https://github.com/PolecatWorks/Graphrag)**: Autonomous 3-Layer Semantic Ontology Graph & Multi-Hop Join-Path Planner.
5. **[`data-engine`](https://github.com/PolecatWorks/data-engine)**: Universal Connectors, Statistical Profiler, Empirical Join Prober, Apache DataFusion Query Engine, Zero-Copy Arrow Python Sandbox, Cryptographic Provenance Ledger & Steward MFE Forms.

---

## Cross-Repository Integration Topology

```mermaid
flowchart TB
    subgraph FrontendTier["1. Micro-Frontend & Shared Canvas Tier (Native Federation)"]
        Shell["Mfe-shell (:4200)\nHost Router + @polecatworks/mfe-shared\n(SharedContextService & SharedCanvasService)"]
        AngryUI["Angry-agent UI (:3001)\nLHS Chat + RHS Live Shared Canvas (MfeRenderer)"]
        DE_FE["data-engine MFE (:3003)\n• ./DataAssetFormWrapper\n• ./JoinHypothesisReviewWrapper\n• ./AlgorithmSandboxWrapper\n• ./ProvenanceLineageWrapper\n• ./StatisticalDiagnosticsWrapper\n• ./DataVisualExplorerWrapper\n• ./SemanticTopologyWrapper"]
        MFE1["mfe1 (:3000)\n• ./DataShowWrapper\n• ./MermaidShowWrapper\n• ./PersonalDataFormWrapper"]
        AAD_FE["agent-as-data UI\nDeclarative Agent & Knowledge Studio"]
    end

    subgraph AgentTier["2. Agent Orchestration & Control Plane Tier"]
        AngryBE["Angry-agent BE\n(LangGraph + Generic Canvas Tools\ninspect_active_mfe / patch_active_mfe / switch_active_mfe)"]
        AAD_BE["agent-as-data BE (:8080)\n(SemanticSteward & ProvenanceDataAnalyst Traits,\nEpistemic Guardrails & Skill Promotion)"]
    end

    subgraph DataAndGraphTier["3. Semantic Graph, Compute & Sandbox Tier (MCP JSON-RPC 2.0)"]
        GR_BE["GraphRAG-RS (/api/graphrag/v1/mcp)\n(Layer 2 Concepts, Layer 1 Datasets, Layer 0 Fields,\ngraphrag_find_join_path, ontology_sync_dataset & ontology/subgraph)"]
        DE_BE["data-engine BE (/api/data-engine/v1/mcp)\n(S3/Parquet/SQL/NoSQL Connectors, MinHash Profiler,\nDataFusion AST/Join Prober, Arrow /dev/shm Python Sandbox,\nVisuals & Topology Engine, W3C PROV Provenance Ledger)"]
    end

    Shell --> AngryUI & DE_FE & MFE1 & AAD_FE
    AngryUI <-->|"MfeCanvasController\n(RFC 6902 JSON Patch + Focus Sync)"| DE_FE & MFE1
    AngryUI <--> AngryBE
    AAD_FE <--> AAD_BE
    AngryBE <-->|"MCP JSON-RPC 2.0"| GR_BE & DE_BE & AAD_BE
    AAD_BE <-->|"MCP JSON-RPC 2.0"| GR_BE & DE_BE
    DE_BE <-->|"ontology_sync_dataset & GET /ontology/subgraph"| GR_BE
```

---

## End-to-End Cross-Repo Sequence Flows

### Flow 1: Autonomous Semantic Discovery & Human-in-the-Loop Verification
```mermaid
sequenceDiagram
    autonumber
    actor Steward as Human Data Steward
    participant Canvas as Angry-agent + de-fe-container (RHS Canvas)
    participant Agent as Agent (Angry-agent / agent-as-data)
    participant DE as data-engine (de-be-container)
    participant GR as Graphrag (graphrag-container)

    Steward->>Canvas: "Ingest s3://lake/orders.parquet & postgres://crm/accounts and discover how they relate"
    Canvas->>Agent: POST /api/chat
    Agent->>DE: MCP: dataset_add & dataset_profile (orders, accounts)
    DE->>DE: Infer Arrow schemas, compute null/distinct stats & MinHash sketches
    DE->>DE: Detect high MinHash overlap on orders.acct_id <-> accounts.id
    DE->>DE: Run empirical DataFusion join probe (99.4% inclusion, N:1 cardinality)
    DE->>GR: MCP: ontology_sync_dataset (status = "empirically_tested", confidence = 0.94)
    Agent->>Canvas: switch_active_mfe("data-engine", "./JoinHypothesisReviewWrapper", empiricalProof)
    Steward->>Canvas: Reviews sample rows & clicks [Approve & Verify] (or edits filter_invariant)
    Canvas->>DE: PUT /api/v1/join-hypotheses/:id (epistemic_status = "verified", verified_by = Steward)
    DE->>GR: Sync verified JOINABLE_WITH tuple
```

### Flow 2: Multi-Dataset Reasoning, Sandboxed Algorithm Execution & Provenance Audit
```mermaid
sequenceDiagram
    autonumber
    actor User as Analyst User
    participant Canvas as Angry-agent + RHS Canvas
    participant Agent as ProvenanceDataAnalyst Agent
    participant GR as Graphrag (/api/graphrag/v1/mcp)
    participant DE as data-engine (/api/data-engine/v1/mcp)

    User->>Canvas: "Build a cohort retention model joining orders and accounts and plot the curve"
    Canvas->>Agent: POST /api/chat (with live canvas state & selection)
    Agent->>GR: MCP: graphrag_find_join_path(["orders", "accounts", "cohort_retention"])
    GR-->>Agent: Verified N:1 join path + BusinessMetric formula + fan_out_warnings = []
    Agent->>DE: MCP: algorithm_add & algorithm_validate (100-row dry-run in Python sandbox)
    DE-->>Agent: Validation passed (output Arrow schema verified)
    Agent->>DE: MCP: algorithm_execute (DataFusion -> /dev/shm Arrow -> Sandboxed Python -> ProvenanceRun)
    DE-->>Agent: Output rows + provenance_run_id + provenance_signature_sha256 + trust_summary
    Agent->>Canvas: Mount chart (./DataVisualExplorerWrapper) + Lineage DAG (./ProvenanceLineageWrapper)
```

### Flow 3: Semantic & Structural Topology Inspection to Interactive Data Visualization
```mermaid
sequenceDiagram
    autonumber
    actor User as Analyst / Steward
    participant Canvas as Angry-agent + RHS Canvas
    participant Agent as SemanticSteward / DataAnalyst Agent
    participant DE as data-engine (/api/data-engine/v1)
    participant GR as Graphrag (/api/graphrag/v1)

    User->>Canvas: "Show how Net Revenue connects to our tables, highlight traps, and plot daily revenue"
    Canvas->>Agent: POST /api/chat
    Agent->>DE: MCP: topology_graph(focus_node_id="net_revenue", hops=2, include_semantics=true)
    DE->>GR: GET /ontology/subgraph?focus_node_id=net_revenue&hops=2
    GR-->>DE: Layer 2 Concepts/Metrics/Communities + Confounders
    DE-->>Agent: Fused SemanticTopologyGraph (with FORBIDDEN_JOIN, drift & fan-out annotations)
    Agent->>Canvas: Mount ./SemanticTopologyWrapper (3-layer view + highlighted join path)
    User->>Canvas: Clicks join edge -> inspects Field Anatomy Venn/Sankey overlap -> clicks [Explore Joined Data]
    Agent->>DE: MCP: visual_render_spec(validated SQL + Vega-Lite DataVisualSpec)
    DE-->>Agent: Bounded aggregated series + ProvenanceRun + trust_summary
    Agent->>Canvas: Mount ./DataVisualExplorerWrapper (live-patchable Vega-Lite / ECharts visual)
```

---

## Related PRDs Across the Ecosystem
- [Backend Foundations PRD](./backend-foundations-prd.md)
- [Universal Connectors, Schema Profiling & Autonomous Semantic Discovery PRD](./data-connectors-profiling-prd.md)
- [Federated Compute, Python Sandbox & Cryptographic Provenance PRD](./compute-sandbox-provenance-prd.md)
- [REST BREAD & MCP Service PRD](./data-engine-mcp-api-prd.md)
- [Angular Native Federation MFE & Bidirectional Shared Canvas PRD](./mfe-frontend-canvas-prd.md)
- [GitHub CI, Multi-Arch Docker Publish & Ephemeral Integration Testing PRD](./ci-cd-container-integration-prd.md)
- [Validation Datasets, Benchmark Harness & Chaos Seeding PRD](./validation-datasets-and-benchmark-prd.md)
- [Statistical Analytics Engine, Diagnostics & Statistical Traps PRD](./statistical-analytics-engine-prd.md)
- [Data Visualizations & Semantic Relationship Topology PRD](./data-visuals-and-semantic-topology-prd.md)
