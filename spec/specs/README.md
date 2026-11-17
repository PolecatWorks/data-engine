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
