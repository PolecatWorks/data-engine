use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::join_probe::probe_join;
use crate::profiler::{DatasetProfile, profile_parquet_dataset};
use crate::seeder::seed_validation_suite;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SeedRequest {
    pub output_dir: String,
    pub seed: Option<u64>,
    pub row_count: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileRequest {
    pub dataset_name: String,
    pub parquet_path: String,
    pub baseline_profile: Option<DatasetProfile>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinProbeRequest {
    pub left_dataset: String,
    pub left_parquet: String,
    pub left_field: String,
    pub right_dataset: String,
    pub right_parquet: String,
    pub right_field: String,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/datasets/seed", post(seed_handler))
        .route("/api/v1/datasets/profile", post(profile_handler))
        .route("/api/v1/joins/probe", post(join_probe_handler))
        .with_state(state)
}

async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: state.config.service.name.clone(),
        version: state.config.service.version.clone(),
    })
}

async fn seed_handler(
    State(state): State<AppState>,
    Json(req): Json<SeedRequest>,
) -> impl IntoResponse {
    let seed = req.seed.unwrap_or(state.config.seeder.default_seed);
    let row_count = req
        .row_count
        .unwrap_or(state.config.seeder.default_row_count);
    let out_dir = PathBuf::from(req.output_dir);

    match seed_validation_suite(seed, row_count, &out_dir) {
        Ok(manifest) => (StatusCode::OK, Json(serde_json::to_value(manifest).unwrap_or_default())),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn profile_handler(
    State(state): State<AppState>,
    Json(req): Json<ProfileRequest>,
) -> impl IntoResponse {
    let parquet_path = PathBuf::from(&req.parquet_path);
    match profile_parquet_dataset(
        &req.dataset_name,
        &parquet_path,
        &state.config.profiler,
        req.baseline_profile.as_ref(),
    ) {
        Ok(profile) => (StatusCode::OK, Json(serde_json::to_value(profile).unwrap_or_default())),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn join_probe_handler(
    State(state): State<AppState>,
    Json(req): Json<JoinProbeRequest>,
) -> impl IntoResponse {
    let left_path = PathBuf::from(&req.left_parquet);
    let right_path = PathBuf::from(&req.right_parquet);
    match probe_join(
        &req.left_dataset,
        &left_path,
        &req.left_field,
        &req.right_dataset,
        &right_path,
        &req.right_field,
        &state.config.compute,
    ) {
        Ok(res) => (StatusCode::OK, Json(serde_json::to_value(res).unwrap_or_default())),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ComputeConfig, ProfilerConfig, SeederConfig, ServiceConfig};
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState {
            config: Arc::new(AppConfig {
                service: ServiceConfig {
                    name: "data-engine-be".to_string(),
                    version: "0.1.0".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 8080,
                    log_level: "info".to_string(),
                },
                profiler: ProfilerConfig {
                    max_sample_rows: 50_000,
                    top_k_values: 5,
                    minhash_num_hashes: 64,
                    drift_relative_mean_threshold: 5.0,
                },
                compute: ComputeConfig {
                    query_timeout_secs: 30,
                    max_preview_rows: 100,
                    join_inclusion_threshold: 0.80,
                },
                seeder: SeederConfig {
                    default_seed: 42,
                    default_row_count: 80,
                },
            }),
        }
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_ok() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("call health");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let payload: HealthResponse = serde_json::from_slice(&bytes).expect("parse health");
        assert_eq!(payload.status, "ok");
        assert_eq!(payload.service, "data-engine-be");
    }

    #[tokio::test]
    async fn test_http_endpoints_end_to_end() {
        let dir = tempdir().expect("tempdir");
        let app = build_router(test_state());

        // 1. Seed via HTTP
        let seed_payload = serde_json::json!({
            "output_dir": dir.path().to_string_lossy(),
            "seed": 42,
            "row_count": 60
        });
        let seed_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/datasets/seed")
                    .header("content-type", "application/json")
                    .body(Body::from(seed_payload.to_string()))
                    .expect("seed request"),
            )
            .await
            .expect("seed response");
        assert_eq!(seed_resp.status(), StatusCode::OK);

        // 2. Profile via HTTP
        let profile_payload = serde_json::json!({
            "dataset_name": "crm_customers",
            "parquet_path": dir.path().join("crm_customers.parquet").to_string_lossy()
        });
        let profile_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/datasets/profile")
                    .header("content-type", "application/json")
                    .body(Body::from(profile_payload.to_string()))
                    .expect("profile request"),
            )
            .await
            .expect("profile response");
        assert_eq!(profile_resp.status(), StatusCode::OK);

        // 3. Join Probe via HTTP (TRAP-02)
        let probe_payload = serde_json::json!({
            "left_dataset": "sales_orders",
            "left_parquet": dir.path().join("sales_orders.parquet").to_string_lossy(),
            "left_field": "cust_ref_key",
            "right_dataset": "crm_customers",
            "right_parquet": dir.path().join("crm_customers.parquet").to_string_lossy(),
            "right_field": "party_uuid"
        });
        let probe_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/joins/probe")
                    .header("content-type", "application/json")
                    .body(Body::from(probe_payload.to_string()))
                    .expect("probe request"),
            )
            .await
            .expect("probe response");
        assert_eq!(probe_resp.status(), StatusCode::OK);
        let probe_bytes = probe_resp
            .into_body()
            .collect()
            .await
            .expect("probe body")
            .to_bytes();
        let probe_json: serde_json::Value =
            serde_json::from_slice(&probe_bytes).expect("parse probe json");
        assert_eq!(probe_json["observed_cardinality"], "N:1");
        assert_eq!(probe_json["epistemic_status"], "empirically_tested");
    }
}
