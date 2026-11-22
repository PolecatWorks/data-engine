# Spec 09: Angular Native Federation MFE & Shared Canvas Remote (`de-fe-container`)

## Status
`complete`

## Overview
Implements `de-fe-container` as specified in [mfe-frontend-canvas-prd.md](../prds/mfe-frontend-canvas-prd.md).
The container is an Angular application configured with `@angular-architects/native-federation` serving as:
1. Standalone Developer & Steward Studio (`:4200`)
2. Federated Remote under `/mfe/data-engine` (`remoteEntry.json`) exposing:
   - `./routes` (Studio navigation)
   - `./DataAssetFormWrapper` (Live Dataset & Field Steward Canvas)
   - `./JoinHypothesisReviewWrapper` (Empirical Join Hypothesis HITL Card)
   - `./AlgorithmSandboxWrapper` (Live SQL/Python Sandbox Execution Canvas)
   - `./ProvenanceLineageWrapper` (Interactive Provenance DAG Canvas)

## Architecture & Requirements
- **Module-Relative Config**: Resolves `config.json` relative to `import.meta.url` to dynamically respect Kubernetes prefix `/mfe/data-engine/config.json`.
- **Canvas Lifecycle Contract**: Wrappers implement `MfeCanvasController` emitting JSON Schema, current state snapshots, focus selection, and accepting RFC 6902 JSON Patches (`applyPatch`).
- **Zero Rewrite Compliance**: Built and packaged for `nginx-view` hosting under `SERVE_ROOT=/mfe/data-engine`.

## Test Strategy
- Vitest unit tests verifying wrapper instantiation, lifecycle (`applyPatch`, `getState`, `getSchema`), and config loading.
