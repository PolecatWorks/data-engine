# Data-Engine Keycloak Service Claims & Auth Addendum PRD

> [!IMPORTANT]
> **Canonical Auth Pattern**: Global Keycloak OIDC (`shell-ui` + `ecosystem-audiences`), `@polecatworks/mfe-shared` token distribution, zero-fake-auth rules, dynamic JWKS verification, and M2M `client_credentials` mechanics are defined canonically in [**Mfe-shell: Ecosystem Keycloak OIDC, Claims Distribution & Service Auth Pattern PRD**](https://github.com/PolecatWorks/Mfe-shell/blob/main/spec/prds/keycloak-oidc-and-claims-distribution-prd.md).
>
> This document specifies only the **Data-Engine (`de-be-container`, `de-fe-container`)** service-local contract.

---

## 1. Keycloak Client & Roles (`data-engine-api`)

- **Client ID (`aud`)**: `data-engine-api`
- **Type**: Bearer-only / Resource Server (declared in `fluxcd-dev/keycloak-crs.yaml`)
- **Client Roles** (`resource_access["data-engine-api"].roles`):
  - `profile:run`: Run data profiling and empirical join-key probes.
  - `query:run`: Execute validated analytical queries against registered datasets.
  - `compute:execute`: Execute Python code and statistical algorithms in the isolated analytic sandbox.
  - `dataset:admin`: Register new connectors, modify lakehouse schema catalogs, and grant access.

---

## 2. Service-Specific Token Claims (`data_engine`)

```json
{
  "sub": "4a761e82-2d17-4861-8290-79883bfad211",
  "aud": ["data-engine-api"],
  "resource_access": {
    "data-engine-api": {
      "roles": ["profile:run", "query:run", "compute:execute"]
    }
  },
  "data_engine": {
    "compute_tier": "standard",
    "max_sandbox_memory_mb": 2048,
    "max_query_runtime_seconds": 60,
    "allowed_catalogs": ["lakehouse_dev", "telemetry"]
  }
}
```

---

## 3. Service-Local Integration Notes

- **Backend (`de-be-container`, Rust/Axum)**: Protects all REST (`/api/data-engine/v1/*`) and MCP (`/mcp`) endpoints via the canonical JWKS middleware (`aud` contains `data-engine-api`), enforcing role gates (`profile:run`, `query:run`, `compute:execute`, `dataset:admin`) and `data_engine` quota claims. Accepts end-user tokens from `de-fe-container` and M2M tokens from `angry-agent-sa` / `agent-as-data-sa`.
- **Frontend (`de-fe-container`)**: Consumes `SharedContextService.getAccessToken()` via `@polecatworks/mfe-shared` per the canonical child MFE pattern.
- **Outbound M2M (`data-engine-sa`)**: Confidential client for background workers publishing lineage records or writing to shared S3/MinIO buckets.
