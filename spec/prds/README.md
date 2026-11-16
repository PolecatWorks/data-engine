# Product Requirements Documents (PRDs) Registry

This directory contains the long-term, persistent Product Requirements Documents (PRDs) defining the architecture, objectives, data models, and capabilities of **Data-Engine (`data-engine`)**.

## Document Hierarchy & Structure

| Document | Description |
| :--- | :--- |
| [backend-foundations-prd.md](./backend-foundations-prd.md) | **Backend Foundations PRD**: Rust backend microservice (`de-be-container`), `clap` CLI, centralized `AppConfig` (`DE_BE__`), modular `thiserror` error hierarchy, configurable Tokio runtime with `tokio-metrics`, PostgreSQL + `pgvector` with reversible migrations, HaMS `:8079` health/metrics sidecar, and Pod-Direct Telemetry Resolution. |
| [data-connectors-profiling-prd.md](./data-connectors-profiling-prd.md) | **Universal Connectors, Schema Profiling & Autonomous Semantic Discovery PRD**: Multi-source ingestion (S3/MinIO Parquet/CSV/JSONL/Iceberg, PostgreSQL/MySQL/DuckDB, MongoDB/OpenSearch NoSQL flattening), statistical column profiling, MinHash value-overlap sketching, empirical join-hypothesis probing, and epistemic synchronization with `Graphrag`. |
| [compute-sandbox-provenance-prd.md](./compute-sandbox-provenance-prd.md) | **Federated Compute, Python Sandbox & Cryptographic Provenance PRD**: Apache DataFusion + `sqlparser-rs` AST/grain validation, fan-out trap detection, zero-copy Apache Arrow shared memory (`/dev/shm`), Rust-supervised isolated Python code sandbox (`NamespaceSandbox` & `SidecarSandbox`), and W3C PROV / OpenLineage provenance ledger. |
| [data-engine-mcp-api-prd.md](./data-engine-mcp-api-prd.md) | **REST BREAD & Model Context Protocol (MCP) Service PRD**: Full REST BREAD APIs (`/sources`, `/datasets`, `/fields`, `/join-hypotheses`, `/queries`, `/algorithms`, `/provenance`) and stateless HTTP JSON-RPC 2.0 MCP server (`/mcp` & `/api/v1/mcp` via `rmcp` + `schemars`) for `agent-as-data` and `Angry-agent`. |
| [mfe-frontend-canvas-prd.md](./mfe-frontend-canvas-prd.md) | **Angular Native Federation MFE & Bidirectional Shared Canvas PRD**: Frontend container (`de-fe-container`), `Mfe-shell` route federation (`./routes`), and bidirectional `MfeCanvasController` wrappers (`./DataAssetFormWrapper`, `./JoinHypothesisReviewWrapper`, `./AlgorithmSandboxWrapper`, `./ProvenanceLineageWrapper`) for live human + AI co-editing on the RHS of chat. |
| [ci-cd-container-integration-prd.md](./ci-cd-container-integration-prd.md) | **GitHub CI, Multi-Arch Docker Publish & Ephemeral Integration Testing PRD**: Multi-stage `cargo-chef` + Nginx container builds, `charts/data-engine` Helm chart, Istio `VirtualService`, path-filtered PR checks, content-SHA multi-arch GHCR builds, and Garden ephemeral Kubernetes Robot Framework integration suites. |

## Related Directories
- [Technical Specifications (Specs)](../specs/README.md) — Derived Test-Driven Development (TDD) implementation contracts.
- [Research Knowledge Base](../research/README.md) — Exploratory technical evaluations and architectural trade-offs.

## Governance & Lifecycle Rules
1. **Source of Truth**: PRD files in this directory define the permanent vision and requirements for `data-engine`.
2. **Iterative Evolution**: As new features or capabilities are conceptualized, PRDs are updated first before deriving specs in `../specs/`.
3. **User Confirmation Gate**: Per [AGENTS.md](../../AGENTS.md), always confirm with the user after updating PRDs before creating or modifying TDD spec files in `../specs/`.
