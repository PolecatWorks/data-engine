# Data-Engine Keycloak Client Claims, Sandbox Security & Service Auth PRD

## Overview
This PRD defines the authentication, claims structure, and machine-to-machine service account policies for **Data-Engine (`de-be-container`, `de-fe-container`)**.

It specifies:
1. **Resource Server Protection (`data-engine-api`)**: Validating OIDC Bearer tokens via Keycloak JWKS for all REST, Arrow Flight, and MCP endpoints.
2. **Domain-Specific Client Claims**: Scoping permissions for data profiling, DataFusion SQL execution, Python sandbox execution, and lakehouse storage access under `resource_access['data-engine-api']`.
3. **M2M Integration with Ecosystem Agents**: Authenticating requests from orchestrators (`angry-agent-sa`, `agent-as-data-sa`) via dedicated service accounts.
4. **Zero Dev/Fake Auth Policy**: Enforcing genuine claims verification in all environments.

---

## Architecture Topology

```mermaid
flowchart TD
    subgraph Ingress ["API & MCP Callers"]
        DE_UI["Data-Engine MFE (de-fe-container)"]
        AA_SA["Angry-Agent SA (angry-agent-sa)"]
        AAD_SA["Agent-As-Data SA (agent-as-data-sa)"]
    end

    subgraph Security ["Data-Engine Auth Middleware"]
        JWKS_Verifier["Keycloak JWKS Verifier"]
        ClaimsExtractor["Claims & Role Gatekeeper"]
    end

    subgraph Core ["Data-Engine Core Capabilities"]
        Profiler["Data Profiler & Join Prober"]
        DataFusion["DataFusion Engine & Query Validator"]
        PySandbox["Python Analytics Sandbox"]
        Provenance["Provenance Ledger"]
    end

    Ingress -->|Bearer Token (aud: data-engine-api)| JWKS_Verifier
    JWKS_Verifier --> ClaimsExtractor
    ClaimsExtractor -->|"role: compute:execute"| PySandbox
    ClaimsExtractor -->|"role: query:run"| DataFusion
    ClaimsExtractor -->|"role: profile:run"| Profiler
    ClaimsExtractor --> Provenance
```

---

## Specification Details

### 1. Keycloak Client Definition (`KeycloakClient: data-engine-api`)
- **Client ID**: `data-engine-api`
- **Type**: Bearer-only / Resource Server
- **Audience**: `aud: ["data-engine-api"]`
- **Client Roles**:
  - `profile:run`: Run data profiling and hypothesis join probes.
  - `query:run`: Execute SQL queries against DataFusion and registered datasets.
  - `compute:execute`: Execute Python code in the containerized analytic sandbox.
  - `dataset:admin`: Register new connectors, modify lakehouse schema catalogs, grant access.

### 2. Unique Token Claims for Data-Engine
```json
{
  "iss": "http://keycloak-keycloakx-http.auth.svc.cluster.local/auth/realms/sw-dev",
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

### 3. Service Account Integration (`data-engine-sa`)
- Confidential client for Data-Engine background workers publishing lineage records to external sinks or writing to shared S3/MinIO buckets.

### 4. Ecosystem Realm Groups (`DataAgentAdmins` & `DataAgentUsers`)
Data-Engine client roles are mapped to users through centralized Keycloak realm groups defined in `Mfe-shell/fluxcd-dev/keycloak-service-capture.yaml`:
- **`DataAgentAdmins` (`metadata.name: data-agent-admins`)**: Grants `profile:run`, `query:run`, `compute:execute`, and `dataset:admin`.
- **`DataAgentUsers` (`metadata.name: data-agent-users`)**: Grants `profile:run`, `query:run`, and `compute:execute`.

