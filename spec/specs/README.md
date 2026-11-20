# Specifications (`spec/specs/`)

This directory contains the detailed technical specifications and Test-Driven Development (TDD) contracts derived from the Product Requirements Documents (`spec/prds/`).

## Spec Status Lifecycle
- **`draft`**: Actively being authored or refined prior to implementation.
- **`complete`**: Marked once the spec features and tests are fully implemented and verified. Cannot be modified except to transition to `deprecated`.
- **`deprecated`**: Marked when a historical spec is superseded.

## Increment 1: Minimal Core (`de-be-container` Scaffold, Chaos Seeder, Profiler & Join Probe)

* [01. Backend Scaffold, Config Loader, Typed Errors & CLI (`de-be-container`)](./01-scaffold-and-config-spec.md) - `complete`
* [02. Deterministic Chaos & Drift Seeder (`seed-validation-suite` & `TRAP-01..04`)](./02-chaos-seeder-and-tpch-spec.md) - `complete`
* [03. Arrow Statistical Profiler & Temporal Drift Detector](./03-arrow-profiler-and-drift-detector-spec.md) - `complete`
* [04. Empirical Join Prober & Fan-Out Detector](./04-empirical-join-probe-spec.md) - `complete`
* [05. Multi-Arch CI/CD Workflows, Helm Chart, Dockerfile & Garden Pipeline (`sward-warden` Pattern)](./05-sward-warden-ci-workflows-spec.md) - `complete`

## Increment 3: Query Plan Validator, Cryptographic Provenance & 3-Mode Epistemic Ablation Runner

* [05. Semantic Query Plan Validator, Safe Aggregation Executor & Cryptographic Provenance (`POST /api/v1/queries/validate`, `POST /api/v1/queries/execute`)](./05-query-validator-and-provenance-spec.md) - `complete`
* [06. 3-Mode Epistemic Ablation Benchmark Runner (`Mode A` vs `Mode B` vs `Mode C`, `POST /api/v1/benchmarks/run`)](./06-ablation-benchmark-runner-spec.md) - `complete`

## Increment 4: Dynamic Python Algorithm Sandbox, `TRAP-05` Analytics & Skill Promotion

* [07. Dynamic Python Algorithm Sandbox, `TRAP-05` Non-SQL Analytics & AAD Skill Promotion (`POST /api/v1/algorithms/validate`, `POST /api/v1/algorithms/execute`)](./07-python-sandbox-and-trap-05-spec.md) - `complete`
* [08. Configurable Service API Prefix & Kubernetes VirtualService Composition (`service.api_prefix`, Istio VirtualService)](./08-configurable-api-prefix-and-virtualservice-spec.md) - `complete`

## Increment 5: Angular Native Federation MFE & Shared Canvas Remote

* [09. Angular Native Federation MFE Remote (`de-fe-container`) & Bidirectional Canvas Wrappers](./09-mfe-frontend-container-spec.md) - `draft`

