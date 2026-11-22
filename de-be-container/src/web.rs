use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::analytics::{
    AnalyticsError, BuiltinAlgorithmRegistry, FieldStatisticalCatalog, StatCorrelationVifRequest,
    StatDescribeRequest, StatHypothesisTestRequest, StatPowerAnalysisRequest,
    StatRegressionFitRequest, StatTimeseriesAnalyzeRequest, run_stat_correlation_vif,
    run_stat_describe, run_stat_hypothesis_test, run_stat_power_analysis, run_stat_regression_fit,
    run_stat_timeseries_analyze,
};
use crate::benchmark::{run_ablation_benchmark, run_statistical_traps_benchmark};
use crate::config::AppConfig;
use crate::join_probe::probe_join;
use crate::profiler::{DatasetProfile, profile_parquet_dataset};
use crate::provenance::ProvenanceStore;
use crate::query_engine::{SemanticQueryPlan, execute_query_plan, validate_query_plan};
use crate::sandbox::{
    PythonAlgorithmExecuteRequest, PythonAlgorithmValidationRequest, execute_python_algorithm,
    validate_python_algorithm,
};
use crate::seeder::seed_validation_suite_with_options;
use crate::topology::{
    JoinStewardUpdateRequest, TopologyCatalog, TopologyGraphQuery, build_dataset_anatomy,
    build_join_anatomy, build_topology_graph, export_topology_mermaid,
};
use crate::visuals::{
    SavedVisualCreateRequest, SavedVisualStore, SavedVisualUpdateRequest, VisualQueryRequest,
    VisualRecommendRequest, VisualsError, execute_visual_query, recommend_visuals,
    recommend_visuals_from_parquet,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub provenance_store: ProvenanceStore,
    pub algorithm_registry: BuiltinAlgorithmRegistry,
    pub field_catalog: FieldStatisticalCatalog,
    pub saved_visuals: SavedVisualStore,
    pub topology_catalog: TopologyCatalog,
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
    #[serde(default)]
    pub include_stat_traps: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FieldVerifyRequest {
    pub verified_by: String,
    #[serde(default)]
    pub recommended_test_family: Option<String>,
    #[serde(default)]
    pub known_confounders: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProfileRequest {
    pub dataset_name: String,
    pub parquet_path: String,
    pub baseline_profile: Option<DatasetProfile>,
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub grain_description: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct SavedVisualListQuery {
    pub dataset_id: Option<String>,
    pub chart_family: Option<String>,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TopologyHttpQuery {
    pub lens: Option<String>,
    pub source_id: Option<String>,
    pub dataset_ids: Option<String>,
    pub epistemic_status: Option<String>,
    pub focus_node_id: Option<String>,
    pub hops: Option<u8>,
    pub include_fields: Option<bool>,
    pub include_forbidden: Option<bool>,
    pub include_semantics: Option<bool>,
    pub provenance_run_id: Option<String>,
}

impl From<TopologyHttpQuery> for TopologyGraphQuery {
    fn from(q: TopologyHttpQuery) -> Self {
        let parse_csv = |opt: Option<String>| -> Option<Vec<String>> {
            opt.and_then(|raw| {
                let items: Vec<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if items.is_empty() { None } else { Some(items) }
            })
        };
        Self {
            lens: q.lens,
            source_id: q.source_id,
            dataset_ids: parse_csv(q.dataset_ids),
            epistemic_status: parse_csv(q.epistemic_status),
            focus_node_id: q.focus_node_id,
            hops: q.hops,
            include_fields: q.include_fields,
            include_forbidden: q.include_forbidden,
            include_semantics: q.include_semantics,
            provenance_run_id: q.provenance_run_id,
        }
    }
}

fn v1_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/datasets", get(datasets_list_handler))
        .route("/datasets/seed", post(seed_handler))
        .route("/datasets/profile", post(profile_handler))
        .route("/joins/probe", post(join_probe_handler))
        .route("/join-hypotheses", get(joins_list_handler))
        .route("/join-hypotheses/{id}", put(join_steward_handler))
        .route("/queries/validate", post(query_validate_handler))
        .route("/queries/execute", post(query_execute_handler))
        .route("/algorithms", get(algorithms_list_handler))
        .route(
            "/algorithms/validate",
            post(algorithm_validate_handler),
        )
        .route(
            "/algorithms/execute",
            post(algorithm_execute_handler),
        )
        .route("/fields", get(fields_list_handler))
        .route(
            "/fields/{canonical_id}",
            get(field_get_handler).put(field_verify_handler),
        )
        .route("/analytics/describe", post(stat_describe_handler))
        .route(
            "/analytics/hypothesis-test",
            post(stat_hypothesis_test_handler),
        )
        .route("/analytics/power", post(stat_power_handler))
        .route("/analytics/correlation", post(stat_correlation_handler))
        .route("/analytics/regression", post(stat_regression_handler))
        .route("/analytics/timeseries", post(stat_timeseries_handler))
        .route("/visuals/recommend", post(visual_recommend_handler))
        .route("/visuals/query", post(visual_query_handler))
        .route(
            "/visuals",
            get(saved_visuals_list_handler).post(saved_visuals_create_handler),
        )
        .route(
            "/visuals/{id}",
            get(saved_visuals_get_handler)
                .put(saved_visuals_update_handler)
                .delete(saved_visuals_delete_handler),
        )
        .route("/topology/graph", get(topology_graph_handler))
        .route(
            "/topology/anatomy/dataset/{id}",
            get(topology_dataset_anatomy_handler),
        )
        .route(
            "/topology/anatomy/join/{id}",
            get(topology_join_anatomy_handler),
        )
        .route("/topology/mermaid", get(topology_mermaid_handler))
        .route("/provenance/{id}", get(provenance_read_handler))
        .route("/benchmarks/run", post(benchmark_run_handler))
        .route("/mcp", post(mcp_rpc_handler))
}

pub fn build_router(state: AppState) -> Router {
    let prefix = state.config.service.api_prefix.trim_end_matches('/');
    let mut router = Router::new()
        .route("/health", get(health_handler))
        .route("/mcp", post(mcp_rpc_handler))
        .nest(prefix, v1_routes());

    if prefix != "/api/v1" {
        router = router.nest("/api/v1", v1_routes());
    }

    router.with_state(state)
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

    match seed_validation_suite_with_options(seed, row_count, &out_dir, req.include_stat_traps) {
        Ok(manifest) => (
            StatusCode::OK,
            Json(serde_json::to_value(manifest).unwrap_or_default()),
        ),
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
        Ok(profile) => {
            let source_name = req
                .source_name
                .unwrap_or_else(|| "local_parquet".to_string());
            let grain = req
                .grain_description
                .unwrap_or_else(|| format!("1 row per {} record", req.dataset_name));
            let _ = state.topology_catalog.upsert_dataset(
                source_name,
                grain,
                Some(req.parquet_path),
                profile.clone(),
            );
            (
                StatusCode::OK,
                Json(serde_json::to_value(profile).unwrap_or_default()),
            )
        }
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
        Ok(res) => {
            let _ = state.topology_catalog.upsert_join_probe(res.clone(), None, None);
            (
                StatusCode::OK,
                Json(serde_json::to_value(res).unwrap_or_default()),
            )
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn query_validate_handler(Json(plan): Json<SemanticQueryPlan>) -> impl IntoResponse {
    let res = validate_query_plan(&plan);
    (
        StatusCode::OK,
        Json(serde_json::to_value(res).unwrap_or_default()),
    )
}

async fn query_execute_handler(
    State(state): State<AppState>,
    Json(plan): Json<SemanticQueryPlan>,
) -> impl IntoResponse {
    match execute_query_plan(&plan, &state.provenance_store) {
        Ok(res) => (
            StatusCode::OK,
            Json(serde_json::to_value(res).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn algorithms_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.algorithm_registry.list() {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!({ "algorithms": items })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn algorithm_validate_handler(
    State(state): State<AppState>,
    Json(req): Json<PythonAlgorithmValidationRequest>,
) -> impl IntoResponse {
    match validate_python_algorithm(&req, &state.config.sandbox) {
        Ok(res) => (
            StatusCode::OK,
            Json(serde_json::to_value(res).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn algorithm_execute_handler(
    State(state): State<AppState>,
    Json(req): Json<PythonAlgorithmExecuteRequest>,
) -> impl IntoResponse {
    match execute_python_algorithm(&req, &state.config.sandbox, &state.provenance_store) {
        Ok(res) => (
            StatusCode::OK,
            Json(serde_json::to_value(res).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn fields_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.field_catalog.list() {
        Ok(items) => (StatusCode::OK, Json(serde_json::json!({ "fields": items }))),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn field_get_handler(
    State(state): State<AppState>,
    Path(canonical_id): Path<String>,
) -> impl IntoResponse {
    match state.field_catalog.get(&canonical_id) {
        Ok(Some(item)) => (
            StatusCode::OK,
            Json(serde_json::to_value(item).unwrap_or_default()),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("field '{}' not found", canonical_id) })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn field_verify_handler(
    State(state): State<AppState>,
    Path(canonical_id): Path<String>,
    Json(req): Json<FieldVerifyRequest>,
) -> impl IntoResponse {
    match state.field_catalog.verify_trait(
        &canonical_id,
        &req.verified_by,
        req.recommended_test_family,
        req.known_confounders,
    ) {
        Ok(Some(updated)) => (
            StatusCode::OK,
            Json(serde_json::to_value(updated).unwrap_or_default()),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("field '{}' not found", canonical_id) })),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn stat_describe_handler(
    State(state): State<AppState>,
    Json(req): Json<StatDescribeRequest>,
) -> impl IntoResponse {
    match run_stat_describe(
        &req,
        &state.config.sandbox,
        &state.provenance_store,
        &state.field_catalog,
    ) {
        Ok(env) => (
            StatusCode::OK,
            Json(serde_json::to_value(env).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn stat_hypothesis_test_handler(
    State(state): State<AppState>,
    Json(req): Json<StatHypothesisTestRequest>,
) -> impl IntoResponse {
    match run_stat_hypothesis_test(
        &req,
        &state.config.sandbox,
        &state.provenance_store,
        &state.field_catalog,
    ) {
        Ok(env) => (
            StatusCode::OK,
            Json(serde_json::to_value(env).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn stat_power_handler(
    State(state): State<AppState>,
    Json(req): Json<StatPowerAnalysisRequest>,
) -> impl IntoResponse {
    match run_stat_power_analysis(
        &req,
        &state.config.sandbox,
        &state.provenance_store,
        &state.field_catalog,
    ) {
        Ok(env) => (
            StatusCode::OK,
            Json(serde_json::to_value(env).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn stat_correlation_handler(
    State(state): State<AppState>,
    Json(req): Json<StatCorrelationVifRequest>,
) -> impl IntoResponse {
    match run_stat_correlation_vif(
        &req,
        &state.config.sandbox,
        &state.provenance_store,
        &state.field_catalog,
    ) {
        Ok(env) => (
            StatusCode::OK,
            Json(serde_json::to_value(env).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn stat_regression_handler(
    State(state): State<AppState>,
    Json(req): Json<StatRegressionFitRequest>,
) -> impl IntoResponse {
    match run_stat_regression_fit(
        &req,
        &state.config.sandbox,
        &state.provenance_store,
        &state.field_catalog,
    ) {
        Ok(env) => (
            StatusCode::OK,
            Json(serde_json::to_value(env).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn stat_timeseries_handler(
    State(state): State<AppState>,
    Json(req): Json<StatTimeseriesAnalyzeRequest>,
) -> impl IntoResponse {
    match run_stat_timeseries_analyze(
        &req,
        &state.config.sandbox,
        &state.provenance_store,
        &state.field_catalog,
    ) {
        Ok(env) => (
            StatusCode::OK,
            Json(serde_json::to_value(env).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn provenance_read_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.provenance_store.get_run(&id) {
        Ok(run) => (
            StatusCode::OK,
            Json(serde_json::to_value(run).unwrap_or_default()),
        ),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

async fn benchmark_run_handler(
    State(state): State<AppState>,
    Json(req): Json<SeedRequest>,
) -> impl IntoResponse {
    let seed = req.seed.unwrap_or(state.config.seeder.default_seed);
    let row_count = req
        .row_count
        .unwrap_or(state.config.seeder.default_row_count);
    let out_dir = PathBuf::from(req.output_dir);

    match run_ablation_benchmark(
        seed,
        row_count,
        &out_dir,
        &state.config.profiler,
        &state.config.compute,
        &state.config.sandbox,
        &state.provenance_store,
    ) {
        Ok(scorecard) => {
            let mut val = serde_json::to_value(scorecard).unwrap_or_default();
            if req.include_stat_traps {
                match run_statistical_traps_benchmark(
                    seed,
                    &out_dir,
                    &state.config.sandbox,
                    &state.provenance_store,
                ) {
                    Ok(stat_card) => {
                        if let Some(obj) = val.as_object_mut() {
                            obj.insert(
                                "statistical_traps".to_string(),
                                serde_json::to_value(stat_card).unwrap_or_default(),
                            );
                        }
                    }
                    Err(err) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({ "error": err.to_string() })),
                        );
                    }
                }
            }
            (StatusCode::OK, Json(val))
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        ),
    }
}

fn map_visuals_error_status(err: &VisualsError) -> StatusCode {
    match err {
        VisualsError::VisualNotFound(_) | VisualsError::TopologyFocusNotFound(_) => {
            StatusCode::NOT_FOUND
        }
        VisualsError::ChartPointLimitExceeded { .. }
        | VisualsError::InvalidVisualSpec(_)
        | VisualsError::UnsupportedRenderer(_)
        | VisualsError::ForbiddenJoinBlocked(_)
        | VisualsError::FanOutTrapBlocked(_)
        | VisualsError::InvalidBinding(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn datasets_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.topology_catalog.list_datasets() {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!({ "datasets": items })),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn joins_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.topology_catalog.list_joins() {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!({ "join_hypotheses": items })),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn join_steward_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinStewardUpdateRequest>,
) -> impl IntoResponse {
    match state.topology_catalog.steward_join(&id, req) {
        Ok(rec) => (
            StatusCode::OK,
            Json(serde_json::to_value(rec).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

fn execute_visual_recommend_inner(
    state: &AppState,
    req: &VisualRecommendRequest,
) -> Result<crate::visuals::VisualRecommendResponse, VisualsError> {
    let mut resp = if let Some(ref prof) = req.profile {
        recommend_visuals(
            prof,
            req.parquet_path.as_deref(),
            &state.field_catalog,
            &state.config.visuals,
            req.renderer.as_deref(),
        )?
    } else if let Some(ref pq) = req.parquet_path {
        let ds_name = req
            .dataset_name
            .as_deref()
            .unwrap_or("dataset");
        recommend_visuals_from_parquet(
            ds_name,
            &PathBuf::from(pq),
            &state.config.profiler,
            &state.field_catalog,
            &state.config.visuals,
            req.renderer.as_deref(),
        )?
    } else if let Some(ref ds_name) = req.dataset_name {
        let rec = state
            .topology_catalog
            .get_dataset(ds_name)?
            .ok_or_else(|| VisualsError::TopologyFocusNotFound(ds_name.clone()))?;
        recommend_visuals(
            &rec.profile,
            rec.parquet_path.as_deref(),
            &state.field_catalog,
            &state.config.visuals,
            req.renderer.as_deref(),
        )?
    } else {
        return Err(VisualsError::InvalidBinding(
            "provide profile, parquet_path, or registered dataset_name".to_string(),
        ));
    };

    if let Some(limit) = req.max_recommendations {
        resp.recommendations.truncate(limit);
    }
    Ok(resp)
}

async fn visual_recommend_handler(
    State(state): State<AppState>,
    Json(req): Json<VisualRecommendRequest>,
) -> impl IntoResponse {
    match execute_visual_recommend_inner(&state, &req) {
        Ok(resp) => (
            StatusCode::OK,
            Json(serde_json::to_value(resp).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

fn execute_visual_query_inner(
    state: &AppState,
    req: &VisualQueryRequest,
) -> Result<crate::visuals::VisualQueryResponse, VisualsError> {
    let resolved_path = if req.data_binding.parquet_path.is_none() {
        req.data_binding
            .dataset_id
            .as_deref()
            .and_then(|id| state.topology_catalog.get_dataset(id).ok().flatten())
            .and_then(|d| d.parquet_path)
            .map(PathBuf::from)
    } else {
        None
    };

    execute_visual_query(
        req,
        resolved_path.as_deref(),
        &state.config.visuals,
        &state.provenance_store,
    )
}

async fn visual_query_handler(
    State(state): State<AppState>,
    Json(req): Json<VisualQueryRequest>,
) -> impl IntoResponse {
    match execute_visual_query_inner(&state, &req) {
        Ok(resp) => (
            StatusCode::OK,
            Json(serde_json::to_value(resp).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn saved_visuals_list_handler(
    State(state): State<AppState>,
    Query(q): Query<SavedVisualListQuery>,
) -> impl IntoResponse {
    match state.saved_visuals.list(
        q.dataset_id.as_deref(),
        q.chart_family.as_deref(),
        q.q.as_deref(),
    ) {
        Ok(items) => (
            StatusCode::OK,
            Json(serde_json::json!({ "visuals": items })),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn saved_visuals_create_handler(
    State(state): State<AppState>,
    Json(req): Json<SavedVisualCreateRequest>,
) -> impl IntoResponse {
    match state.saved_visuals.create(req) {
        Ok(rec) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(rec).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn saved_visuals_get_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.saved_visuals.get(id) {
        Ok(rec) => (
            StatusCode::OK,
            Json(serde_json::to_value(rec).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn saved_visuals_update_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<SavedVisualUpdateRequest>,
) -> impl IntoResponse {
    match state.saved_visuals.update(id, req) {
        Ok(rec) => (
            StatusCode::OK,
            Json(serde_json::to_value(rec).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn saved_visuals_delete_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.saved_visuals.delete(id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn topology_graph_handler(
    State(state): State<AppState>,
    Query(query): Query<TopologyHttpQuery>,
) -> impl IntoResponse {
    let q: TopologyGraphQuery = query.into();
    match build_topology_graph(
        &state.topology_catalog,
        &state.field_catalog,
        &state.provenance_store,
        &state.config.visuals,
        &q,
    )
    .await
    {
        Ok(graph) => (
            StatusCode::OK,
            Json(serde_json::to_value(graph).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn topology_dataset_anatomy_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match build_dataset_anatomy(&id, &state.topology_catalog, &state.field_catalog) {
        Ok(anatomy) => (
            StatusCode::OK,
            Json(serde_json::to_value(anatomy).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn topology_join_anatomy_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match build_join_anatomy(&id, &state.topology_catalog) {
        Ok(anatomy) => (
            StatusCode::OK,
            Json(serde_json::to_value(anatomy).unwrap_or_default()),
        )
            .into_response(),
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn topology_mermaid_handler(
    State(state): State<AppState>,
    Query(query): Query<TopologyHttpQuery>,
) -> impl IntoResponse {
    let q: TopologyGraphQuery = query.into();
    match build_topology_graph(
        &state.topology_catalog,
        &state.field_catalog,
        &state.provenance_store,
        &state.config.visuals,
        &q,
    )
    .await
    {
        Ok(graph) => {
            let resp = export_topology_mermaid(graph);
            (
                StatusCode::OK,
                Json(serde_json::to_value(resp).unwrap_or_default()),
            )
                .into_response()
        }
        Err(err) => (
            map_visuals_error_status(&err),
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

fn mcp_tool_catalog() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "hello",
            "description": "Health-check MCP tool returning a greeting from data-engine.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string" } }
            }
        },
        {
            "name": "algorithm_browse",
            "description": "List all hydrated, SHA-256-pinned built-in statistical algorithms.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "stat_describe_and_check",
            "description": "Compute distributional moments, quantiles, Shapiro-Wilk/D'Agostino normality, outlier counts, and learned field traits.",
            "inputSchema": {
                "type": "object",
                "required": ["input_binding", "columns"],
                "properties": {
                    "input_binding": { "type": "object" },
                    "columns": { "type": "array", "items": { "type": "string" } },
                    "sampling": { "type": "object" }
                }
            }
        },
        {
            "name": "stat_hypothesis_test",
            "description": "Execute parametric/non-parametric hypothesis tests with assumption checks, CIs, BH-FDR correction, and Simpson's paradox stratification.",
            "inputSchema": {
                "type": "object",
                "required": ["input_binding", "method", "value_col"],
                "properties": {
                    "input_binding": { "type": "object" },
                    "method": { "type": "string" },
                    "value_col": { "type": "string" },
                    "group_col": { "type": "string" },
                    "segment_col": { "type": "string" },
                    "stratify_by": { "type": "string" },
                    "correction": { "type": "string" },
                    "alpha": { "type": "number" }
                }
            }
        },
        {
            "name": "stat_power_analysis",
            "description": "Compute prospective/retrospective statistical power, required sample size, or MDE.",
            "inputSchema": {
                "type": "object",
                "required": ["test_type", "solve_for"],
                "properties": {
                    "test_type": { "type": "string" },
                    "solve_for": { "type": "string" },
                    "alpha": { "type": "number" },
                    "power": { "type": "number" },
                    "effect_size": { "type": "number" },
                    "p1": { "type": "number" },
                    "p2": { "type": "number" },
                    "n_per_group": { "type": "integer" }
                }
            }
        },
        {
            "name": "stat_correlation_vif",
            "description": "Compute Pearson/Spearman/Kendall correlation matrices with FDR adjustment, partial correlations, and VIF multicollinearity diagnostics.",
            "inputSchema": {
                "type": "object",
                "required": ["input_binding", "columns"],
                "properties": {
                    "input_binding": { "type": "object" },
                    "columns": { "type": "array", "items": { "type": "string" } },
                    "methods": { "type": "array", "items": { "type": "string" } },
                    "control_cols": { "type": "array", "items": { "type": "string" } },
                    "correction": { "type": "string" }
                }
            }
        },
        {
            "name": "stat_regression_fit",
            "description": "Fit OLS, GLM, or Huber robust regression with HC3 SEs, Breusch-Pagan, Durbin-Watson, and Cook's distance diagnostics.",
            "inputSchema": {
                "type": "object",
                "required": ["input_binding", "model_type", "target_col", "feature_cols"],
                "properties": {
                    "input_binding": { "type": "object" },
                    "model_type": { "type": "string" },
                    "target_col": { "type": "string" },
                    "feature_cols": { "type": "array", "items": { "type": "string" } },
                    "add_intercept": { "type": "boolean" }
                }
            }
        },
        {
            "name": "stat_timeseries_analyze",
            "description": "Perform STL decomposition, ADF/KPSS stationarity, ACF/PACF, spurious cross-series correlation checks, and ETS/ARIMA forecasting.",
            "inputSchema": {
                "type": "object",
                "required": ["input_binding", "time_col", "value_col"],
                "properties": {
                    "input_binding": { "type": "object" },
                    "time_col": { "type": "string" },
                    "value_col": { "type": "string" },
                    "compare_col": { "type": "string" },
                    "period": { "type": "integer" },
                    "forecast_horizon": { "type": "integer" },
                    "model": { "type": "string" }
                }
            }
        },
        {
            "name": "visual_recommend",
            "description": "Deterministically recommend ranked Vega-Lite v5 or Apache ECharts DataVisualSpec candidates from dataset schema and profile statistics.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dataset_name": { "type": "string" },
                    "parquet_path": { "type": "string" },
                    "renderer": { "type": "string", "enum": ["vega_lite", "echarts"] },
                    "max_recommendations": { "type": "integer" }
                }
            }
        },
        {
            "name": "visual_render_spec",
            "description": "Validate semantic/fan-out guardrails, execute bounded aggregation or seeded sampling with ProvenanceRun tracking, and return a canvas-ready DataVisualSpec for ./DataVisualExplorerWrapper.",
            "inputSchema": {
                "type": "object",
                "required": ["data_binding", "spec"],
                "properties": {
                    "title": { "type": "string" },
                    "renderer": { "type": "string", "enum": ["vega_lite", "echarts"] },
                    "chart_family": { "type": "string" },
                    "data_binding": { "type": "object" },
                    "spec": { "type": "object" },
                    "sampling": { "type": "object" },
                    "question": { "type": "string" }
                }
            }
        },
        {
            "name": "topology_graph",
            "description": "Return a bounded multi-layer SemanticTopologyGraph (Business Concepts/Metrics -> Datasets -> Fields, JOINABLE_WITH, FORBIDDEN_JOIN, fan-out & CONFOUNDED_BY overlays) for ./SemanticTopologyWrapper.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lens": { "type": "string", "enum": ["semantic_3layer", "er_join", "lineage_overlay"] },
                    "focus_node_id": { "type": "string" },
                    "hops": { "type": "integer" },
                    "dataset_ids": { "type": "array", "items": { "type": "string" } },
                    "include_fields": { "type": "boolean" },
                    "include_forbidden": { "type": "boolean" },
                    "include_semantics": { "type": "boolean" },
                    "provenance_run_id": { "type": "string" }
                }
            }
        },
        {
            "name": "topology_field_anatomy",
            "description": "Return zoomed-in nested Arrow schema anatomy for a dataset or side-by-side schema cards, type compatibility, Venn/Sankey overlap, and fan-out diagnostics for a join hypothesis.",
            "inputSchema": {
                "type": "object",
                "required": ["target_kind", "target_id"],
                "properties": {
                    "target_kind": { "type": "string", "enum": ["dataset", "join"] },
                    "target_id": { "type": "string" }
                }
            }
        }
    ])
}

#[derive(Debug, Error)]
pub enum McpError {
    #[error(transparent)]
    Analytics(#[from] AnalyticsError),

    #[error(transparent)]
    Visuals(#[from] VisualsError),

    #[error("invalid MCP tool arguments or payload: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Unknown MCP tool '{0}'")]
    UnknownTool(String),
}

async fn dispatch_mcp_tool(
    state: &AppState,
    name: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    match name {
        "hello" => {
            let who = args
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("world")
                .trim();
            Ok(serde_json::json!({ "message": format!("Hello from data-engine, {}!", who) }))
        }
        "algorithm_browse" => {
            let list = state.algorithm_registry.list()?;
            Ok(serde_json::json!({ "algorithms": list }))
        }
        "stat_describe_and_check" => {
            let req: StatDescribeRequest = serde_json::from_value(args)?;
            let env = run_stat_describe(
                &req,
                &state.config.sandbox,
                &state.provenance_store,
                &state.field_catalog,
            )?;
            Ok(serde_json::to_value(env)?)
        }
        "stat_hypothesis_test" => {
            let req: StatHypothesisTestRequest = serde_json::from_value(args)?;
            let env = run_stat_hypothesis_test(
                &req,
                &state.config.sandbox,
                &state.provenance_store,
                &state.field_catalog,
            )?;
            Ok(serde_json::to_value(env)?)
        }
        "stat_power_analysis" => {
            let req: StatPowerAnalysisRequest = serde_json::from_value(args)?;
            let env = run_stat_power_analysis(
                &req,
                &state.config.sandbox,
                &state.provenance_store,
                &state.field_catalog,
            )?;
            Ok(serde_json::to_value(env)?)
        }
        "stat_correlation_vif" => {
            let req: StatCorrelationVifRequest = serde_json::from_value(args)?;
            let env = run_stat_correlation_vif(
                &req,
                &state.config.sandbox,
                &state.provenance_store,
                &state.field_catalog,
            )?;
            Ok(serde_json::to_value(env)?)
        }
        "stat_regression_fit" => {
            let req: StatRegressionFitRequest = serde_json::from_value(args)?;
            let env = run_stat_regression_fit(
                &req,
                &state.config.sandbox,
                &state.provenance_store,
                &state.field_catalog,
            )?;
            Ok(serde_json::to_value(env)?)
        }
        "stat_timeseries_analyze" => {
            let req: StatTimeseriesAnalyzeRequest = serde_json::from_value(args)?;
            let env = run_stat_timeseries_analyze(
                &req,
                &state.config.sandbox,
                &state.provenance_store,
                &state.field_catalog,
            )?;
            Ok(serde_json::to_value(env)?)
        }
        "visual_recommend" => {
            let req: VisualRecommendRequest = serde_json::from_value(args)?;
            let resp = execute_visual_recommend_inner(state, &req)?;
            Ok(serde_json::to_value(resp)?)
        }
        "visual_render_spec" => {
            let req: VisualQueryRequest = serde_json::from_value(args)?;
            let resp = execute_visual_query_inner(state, &req)?;
            Ok(serde_json::to_value(resp)?)
        }
        "topology_graph" => {
            let req: TopologyGraphQuery = serde_json::from_value(args)?;
            let graph = build_topology_graph(
                &state.topology_catalog,
                &state.field_catalog,
                &state.provenance_store,
                &state.config.visuals,
                &req,
            )
            .await?;
            Ok(serde_json::to_value(graph)?)
        }
        "topology_field_anatomy" => {
            let target_kind = args
                .get("target_kind")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    VisualsError::InvalidVisualSpec("missing target_kind ('dataset' or 'join')".to_string())
                })?;
            let target_id = args
                .get("target_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    VisualsError::InvalidVisualSpec("missing target_id".to_string())
                })?;
            match target_kind {
                "dataset" => {
                    let resp = build_dataset_anatomy(
                        target_id,
                        &state.topology_catalog,
                        &state.field_catalog,
                    )?;
                    Ok(serde_json::to_value(resp)?)
                }
                "join" => {
                    let resp = build_join_anatomy(target_id, &state.topology_catalog)?;
                    Ok(serde_json::to_value(resp)?)
                }
                other => Err(McpError::Visuals(VisualsError::InvalidVisualSpec(format!(
                    "unsupported target_kind '{}'; expected 'dataset' or 'join'",
                    other
                )))),
            }
        }
        other => Err(McpError::UnknownTool(other.to_string())),
    }
}

fn mcp_tool_result(
    id: serde_json::Value,
    value: Result<serde_json::Value, McpError>,
) -> Json<serde_json::Value> {
    match value {
        Ok(payload) => {
            let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string());
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": false
                }
            }))
        }
        Err(err) => Json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": err.to_string() }],
                "isError": true
            }
        })),
    }
}

async fn mcp_rpc_handler(
    State(state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let Some(method) = req.get("method").and_then(|m| m.as_str()) else {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32600, "message": "Missing JSON-RPC method" }
            })),
        );
    };

    match method {
        "initialize" => (
            StatusCode::OK,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": state.config.service.name,
                        "version": state.config.service.version
                    }
                }
            })),
        ),
        "notifications/initialized" | "ping" => (
            StatusCode::OK,
            Json(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
        ),
        "tools/list" => (
            StatusCode::OK,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": mcp_tool_catalog() }
            })),
        ),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or_default();
            let Some(name) = params.get("name").and_then(|n| n.as_str()) else {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32602, "message": "Missing params.name" }
                    })),
                );
            };
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            let out = dispatch_mcp_tool(&state, name, args).await;
            (StatusCode::OK, mcp_tool_result(id, out))
        }
        other => (
            StatusCode::OK,
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method '{}' not found", other) }
            })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ComputeConfig, ProfilerConfig, SandboxBackend, SandboxConfig, SeederConfig, ServiceConfig,
    };
    use crate::sandbox::detect_test_python_binary;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let sandbox = SandboxConfig {
            backend: SandboxBackend::Process,
            python_binary: detect_test_python_binary(),
            timeout_ms: 20_000,
            max_code_bytes: 65_536,
            max_sandbox_rows: 100_000,
            max_plot_points: 1000,
            shm_dir: if std::path::Path::new("/dev/shm").exists() {
                "/dev/shm".to_string()
            } else {
                std::env::temp_dir().to_string_lossy().to_string()
            },
            allowed_modules: vec![
                "math".to_string(),
                "statistics".to_string(),
                "json".to_string(),
                "numpy".to_string(),
                "scipy".to_string(),
                "statsmodels".to_string(),
                "pyarrow".to_string(),
                "pyarrow.ipc".to_string(),
            ],
        };
        let algorithm_registry = BuiltinAlgorithmRegistry::default();
        algorithm_registry
            .hydrate_builtins(&sandbox)
            .expect("hydrate builtins for web tests");

        AppState {
            config: Arc::new(AppConfig {
                service: ServiceConfig {
                    name: "data-engine-be".to_string(),
                    version: "0.1.0".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 8080,
                    log_level: "info".to_string(),
                    api_prefix: "/api/data-engine/v1".to_string(),
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
                sandbox,
                visuals: crate::visuals::config::VisualsConfig::default(),
            }),
            provenance_store: ProvenanceStore::default(),
            algorithm_registry,
            field_catalog: FieldStatisticalCatalog::default(),
            saved_visuals: SavedVisualStore::default(),
            topology_catalog: TopologyCatalog::default(),
        }
    }

    #[tokio::test]
    async fn test_health_endpoint_returns_ok() {
        let app = build_router(test_state());

        // Test root health probe
        let resp_root = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("call /health");
        assert_eq!(resp_root.status(), StatusCode::OK);

        // Test backward-compatible /api/v1/health
        let response = app
            .clone()
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

        // Test configured prefix /api/data-engine/v1/health
        let resp_prefix = app
            .oneshot(
                Request::builder()
                    .uri("/api/data-engine/v1/health")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("call /api/data-engine/v1/health");
        assert_eq!(resp_prefix.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_custom_api_prefix_routing() {
        let mut state = test_state();
        let mut cfg = (*state.config).clone();
        cfg.service.api_prefix = "/api/custom/v1".to_string();
        state.config = Arc::new(cfg);

        let app = build_router(state);

        // Custom prefix works
        let resp_custom = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/custom/v1/health")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("call custom");
        assert_eq!(resp_custom.status(), StatusCode::OK);

        // Backward compatibility /api/v1 works
        let resp_v1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("call v1");
        assert_eq!(resp_v1.status(), StatusCode::OK);

        // Root /health works
        let resp_root = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("call root");
        assert_eq!(resp_root.status(), StatusCode::OK);
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
            .clone()
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

        // 4. Run 3-Mode Ablation Benchmark via HTTP
        let bench_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/benchmarks/run")
                    .header("content-type", "application/json")
                    .body(Body::from(seed_payload.to_string()))
                    .expect("bench request"),
            )
            .await
            .expect("bench response");
        assert_eq!(bench_resp.status(), StatusCode::OK);
        let bench_bytes = bench_resp
            .into_body()
            .collect()
            .await
            .expect("bench body")
            .to_bytes();
        let bench_json: serde_json::Value =
            serde_json::from_slice(&bench_bytes).expect("parse bench json");
        assert_eq!(
            bench_json["mode_c_autonomous_plus_hitl_verified"]["execution_accuracy_pct"],
            100.0
        );
    }

    #[tokio::test]
    async fn test_statistical_rest_and_mcp_endpoints_end_to_end() {
        let dir = tempdir().expect("tempdir");
        let app = build_router(test_state());

        // 1. List hydrated built-in algorithms
        let alg_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/algorithms")
                    .body(Body::empty())
                    .expect("algorithms req"),
            )
            .await
            .expect("algorithms resp");
        assert_eq!(alg_resp.status(), StatusCode::OK);
        let alg_bytes = alg_resp.into_body().collect().await.expect("body").to_bytes();
        let alg_json: serde_json::Value = serde_json::from_slice(&alg_bytes).expect("json");
        assert_eq!(alg_json["algorithms"].as_array().expect("arr").len(), 6);

        // 2. Seed with STAT-TRAP-01..04
        let seed_payload = serde_json::json!({
            "output_dir": dir.path().to_string_lossy(),
            "seed": 42,
            "row_count": 60,
            "include_stat_traps": true
        });
        let seed_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/datasets/seed")
                    .header("content-type", "application/json")
                    .body(Body::from(seed_payload.to_string()))
                    .expect("seed req"),
            )
            .await
            .expect("seed resp");
        assert_eq!(seed_resp.status(), StatusCode::OK);
        let seed_bytes = seed_resp.into_body().collect().await.expect("body").to_bytes();
        let seed_json: serde_json::Value = serde_json::from_slice(&seed_bytes).expect("json");
        assert_eq!(seed_json["stat_traps"].as_array().expect("arr").len(), 4);

        // 3. Call /api/v1/analytics/describe on orders_heavy_tail.parquet
        let desc_payload = serde_json::json!({
            "input_binding": {
                "dataset_name": "orders_heavy_tail",
                "parquet_path": dir.path().join("orders_heavy_tail.parquet").to_string_lossy()
            },
            "columns": ["order_total_usd"]
        });
        let desc_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/describe")
                    .header("content-type", "application/json")
                    .body(Body::from(desc_payload.to_string()))
                    .expect("describe req"),
            )
            .await
            .expect("describe resp");
        assert_eq!(desc_resp.status(), StatusCode::OK);
        let desc_bytes = desc_resp.into_body().collect().await.expect("body").to_bytes();
        let desc_json: serde_json::Value = serde_json::from_slice(&desc_bytes).expect("json");
        assert_eq!(desc_json["tool"], "stat_describe_and_check");
        assert!(!desc_json["provenance_run_id"].as_str().unwrap_or("").is_empty());

        // 4. Verify learned field trait in /api/v1/fields/{canonical_id}
        let verify_payload = serde_json::json!({
            "verified_by": "steward@example.com",
            "recommended_test_family": "mann_whitney_u",
            "known_confounders": ["enterprise_whale_tier"]
        });
        let verify_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/fields/orders_heavy_tail.order_total_usd")
                    .header("content-type", "application/json")
                    .body(Body::from(verify_payload.to_string()))
                    .expect("verify req"),
            )
            .await
            .expect("verify resp");
        assert_eq!(verify_resp.status(), StatusCode::OK);
        let verify_bytes = verify_resp.into_body().collect().await.expect("body").to_bytes();
        let verify_json: serde_json::Value = serde_json::from_slice(&verify_bytes).expect("json");
        assert_eq!(verify_json["epistemic_status"], "verified");
        assert_eq!(verify_json["verified_by"], "steward@example.com");

        // 5. MCP tools/list & tools/call (stat_power_analysis)
        let mcp_list_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        });
        let mcp_list_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(mcp_list_req.to_string()))
                    .expect("mcp list req"),
            )
            .await
            .expect("mcp list resp");
        assert_eq!(mcp_list_resp.status(), StatusCode::OK);
        let mcp_list_bytes = mcp_list_resp.into_body().collect().await.expect("body").to_bytes();
        let mcp_list_json: serde_json::Value = serde_json::from_slice(&mcp_list_bytes).expect("json");
        let tools = mcp_list_json["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 12);
        let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in [
            "algorithm_browse",
            "stat_describe_and_check",
            "stat_hypothesis_test",
            "stat_power_analysis",
            "stat_correlation_vif",
            "stat_regression_fit",
            "stat_timeseries_analyze",
        ] {
            assert!(tool_names.contains(&expected), "missing MCP tool {}", expected);
        }

        let mcp_call_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "stat_power_analysis",
                "arguments": {
                    "test_type": "two_sample_t",
                    "solve_for": "sample_size",
                    "alpha": 0.05,
                    "power": 0.80,
                    "effect_size": 0.5
                }
            }
        });
        let mcp_call_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(mcp_call_req.to_string()))
                    .expect("mcp call req"),
            )
            .await
            .expect("mcp call resp");
        assert_eq!(mcp_call_resp.status(), StatusCode::OK);
        let mcp_call_bytes = mcp_call_resp.into_body().collect().await.expect("body").to_bytes();
        let mcp_call_json: serde_json::Value = serde_json::from_slice(&mcp_call_bytes).expect("json");
        assert_eq!(mcp_call_json["result"]["isError"], false);
        let content_text = mcp_call_json["result"]["content"][0]["text"]
            .as_str()
            .expect("content text");
        let envelope: serde_json::Value = serde_json::from_str(content_text).expect("parse env");
        let req_n = envelope["primary_result"]["required_n_per_group"]
            .as_u64()
            .expect("required_n_per_group");
        assert!((63..=65).contains(&req_n), "expected n ~ 64, got {}", req_n);

        // 6. Run benchmark with include_stat_traps: true
        let bench_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/benchmarks/run")
                    .header("content-type", "application/json")
                    .body(Body::from(seed_payload.to_string()))
                    .expect("bench req"),
            )
            .await
            .expect("bench resp");
        assert_eq!(bench_resp.status(), StatusCode::OK);
        let bench_bytes = bench_resp.into_body().collect().await.expect("body").to_bytes();
        let bench_json: serde_json::Value = serde_json::from_slice(&bench_bytes).expect("json");
        assert_eq!(
            bench_json["statistical_traps"]["full_protocol_accuracy_pct"],
            100.0
        );
    }

    #[tokio::test]
    async fn test_visuals_and_topology_http_and_mcp_endpoints() {
        let dir = tempdir().unwrap();
        let state = test_state();
        let app = build_router(state);

        // 1. Seed validation suite
        let seed_payload = serde_json::json!({
            "output_dir": dir.path().to_string_lossy(),
            "seed": 42,
            "row_count": 40
        });
        let seed_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data-engine/v1/datasets/seed")
                    .header("content-type", "application/json")
                    .body(Body::from(seed_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(seed_resp.status(), StatusCode::OK);

        // 2. Profile orders & customers (populates TopologyCatalog)
        let orders_pq = dir.path().join("sales_orders.parquet");
        let customers_pq = dir.path().join("crm_customers.parquet");
        for (name, path) in [("orders", &orders_pq), ("customers", &customers_pq)] {
            let p_resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/data-engine/v1/datasets/profile")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "dataset_name": name,
                                "parquet_path": path.to_string_lossy()
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(p_resp.status(), StatusCode::OK);
        }

        // 3. Probe join orders.cust_ref_key <-> customers.party_uuid
        let probe_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data-engine/v1/joins/probe")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "left_dataset": "orders",
                            "left_parquet": orders_pq.to_string_lossy(),
                            "left_field": "cust_ref_key",
                            "right_dataset": "customers",
                            "right_parquet": customers_pq.to_string_lossy(),
                            "right_field": "party_uuid"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(probe_resp.status(), StatusCode::OK);

        // 4. Recommend visuals via REST
        let rec_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data-engine/v1/visuals/recommend")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "dataset_name": "orders" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rec_resp.status(), StatusCode::OK);
        let rec_json: serde_json::Value =
            serde_json::from_slice(&rec_resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert!(!rec_json["recommendations"].as_array().unwrap().is_empty());

        // 5. Execute visual query via REST and verify ProvenanceRun
        let vq_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data-engine/v1/visuals/query")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "title": "Orders Histogram",
                            "renderer": "vega_lite",
                            "chart_family": "histogram",
                            "data_binding": { "kind": "dataset", "dataset_id": "orders" },
                            "spec": { "mark": "bar", "encoding": {} }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vq_resp.status(), StatusCode::OK);
        let vq_json: serde_json::Value =
            serde_json::from_slice(&vq_resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(vq_json["provenance"]["rendered_rows"], 80);
        assert!(vq_json["provenance"]["provenance_run_id"].as_str().unwrap().starts_with("prov-"));

        // 6. GET /topology/graph, /topology/anatomy/dataset/orders, /topology/mermaid
        let topo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/data-engine/v1/topology/graph?lens=semantic_3layer")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(topo_resp.status(), StatusCode::OK);
        let topo_json: serde_json::Value =
            serde_json::from_slice(&topo_resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert!(topo_json["nodes"].as_array().unwrap().len() >= 4);

        let anat_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/data-engine/v1/topology/anatomy/dataset/orders")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anat_resp.status(), StatusCode::OK);

        let mm_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/data-engine/v1/topology/mermaid?lens=er_join")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mm_resp.status(), StatusCode::OK);

        // 7. MCP tools/list includes all 4 new visual/topology tools & tools/call succeeds
        let list_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data-engine/v1/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"jsonrpc":"2.0","id":10,"method":"tools/list"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let list_json: serde_json::Value =
            serde_json::from_slice(&list_resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
        let tool_names: Vec<&str> = list_json["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for expected in [
            "visual_recommend",
            "visual_render_spec",
            "topology_graph",
            "topology_field_anatomy",
        ] {
            assert!(tool_names.contains(&expected), "missing MCP tool {}", expected);
        }

        let mcp_topo = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data-engine/v1/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": 11,
                            "method": "tools/call",
                            "params": {
                                "name": "topology_field_anatomy",
                                "arguments": { "target_kind": "dataset", "target_id": "orders" }
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let mcp_json: serde_json::Value =
            serde_json::from_slice(&mcp_topo.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(mcp_json["result"]["isError"], false);
    }
}
