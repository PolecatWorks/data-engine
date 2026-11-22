//! `rmcp`-backed Model Context Protocol server for PolecatWorks Data-Engine.
//!
//! Tool metadata and JSON Schema definitions (`inputSchema`) are automatically generated
//! via `rmcp`'s `#[tool]` / `#[tool_router]` macros and `schemars::JsonSchema`.

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::analytics::{
    StatCorrelationVifRequest, StatDescribeRequest, StatHypothesisTestRequest,
    StatPowerAnalysisRequest, StatRegressionFitRequest, StatTimeseriesAnalyzeRequest,
    run_stat_correlation_vif, run_stat_describe, run_stat_hypothesis_test,
    run_stat_power_analysis, run_stat_regression_fit, run_stat_timeseries_analyze,
};
use crate::mcp::McpError;
use crate::topology::{
    TopologyGraphQuery, build_dataset_anatomy, build_join_anatomy, build_topology_graph,
};
use crate::visuals::{VisualQueryRequest, VisualRecommendRequest, VisualsError};
use crate::web::{AppState, execute_visual_query_inner, execute_visual_recommend_inner};

// ============================================================================
// MCP Tool Request Schemas (auto-generated via schemars::JsonSchema)
// ============================================================================

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct HelloMcpRequest {
    #[schemars(description = "Optional name of the caller or entity to greet (defaults to 'world').")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AlgorithmBrowseMcpRequest {}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct StatDescribeMcpRequest {
    #[schemars(description = "Dataset input binding object containing dataset_name and parquet_path.")]
    pub input_binding: Value,
    #[schemars(description = "List of numeric column names to describe and diagnose.")]
    pub columns: Vec<String>,
    #[schemars(description = "Optional seeded sampling specification object.")]
    pub sampling: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct StatHypothesisTestMcpRequest {
    #[schemars(description = "Dataset input binding object containing dataset_name and parquet_path.")]
    pub input_binding: Value,
    #[schemars(description = "Hypothesis test method (e.g. welch_t, mann_whitney_u, anova, kruskal_wallis, chi2_independence).")]
    pub method: String,
    #[schemars(description = "Primary measurement or outcome column name.")]
    pub value_col: String,
    #[schemars(description = "Grouping column name for two-sample or multi-group comparisons.")]
    pub group_col: Option<String>,
    #[schemars(description = "First column name for paired tests.")]
    pub col_a: Option<String>,
    #[schemars(description = "Second column name for paired tests.")]
    pub col_b: Option<String>,
    #[schemars(description = "Segment column name for multi-segment testing with multiple-comparison correction.")]
    pub segment_col: Option<String>,
    #[schemars(description = "Stratification column name for Simpson's paradox detection.")]
    pub stratify_by: Option<String>,
    #[schemars(description = "Multiple-comparison correction method (e.g. none, bonferroni, fdr_bh).")]
    pub correction: Option<String>,
    #[schemars(description = "Alternative hypothesis ('two-sided', 'less', or 'greater').")]
    pub alternative: Option<String>,
    #[schemars(description = "Significance level alpha (default 0.05).")]
    pub alpha: Option<f64>,
    #[schemars(description = "Optional seeded sampling specification object.")]
    pub sampling: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct StatPowerAnalysisMcpRequest {
    #[schemars(description = "Statistical test type (e.g. two_sample_t, two_proportion_z, one_way_anova).")]
    pub test_type: String,
    #[schemars(description = "Quantity to solve for ('sample_size', 'power', or 'effect_size').")]
    pub solve_for: String,
    #[schemars(description = "Significance level alpha (default 0.05).")]
    pub alpha: Option<f64>,
    #[schemars(description = "Target statistical power (1 - beta), e.g. 0.80.")]
    pub power: Option<f64>,
    #[schemars(description = "Standardized effect size (e.g. Cohen's d or f).")]
    pub effect_size: Option<f64>,
    #[schemars(description = "Baseline proportion p1 for proportion tests.")]
    pub p1: Option<f64>,
    #[schemars(description = "Variant proportion p2 for proportion tests.")]
    pub p2: Option<f64>,
    #[schemars(description = "Sample size per group.")]
    pub n_per_group: Option<usize>,
    #[schemars(description = "Group allocation ratio n2/n1 (default 1.0).")]
    pub ratio: Option<f64>,
    #[schemars(description = "Number of groups for ANOVA power calculations (default 2).")]
    pub num_groups: Option<usize>,
    #[schemars(description = "Alternative hypothesis ('two-sided', 'larger', or 'smaller').")]
    pub alternative: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct StatCorrelationVifMcpRequest {
    #[schemars(description = "Dataset input binding object containing dataset_name and parquet_path.")]
    pub input_binding: Value,
    #[schemars(description = "Numeric column names to include in correlation and VIF analysis.")]
    pub columns: Vec<String>,
    #[schemars(description = "Correlation methods to compute (e.g. ['pearson', 'spearman', 'kendall']).")]
    pub methods: Option<Vec<String>>,
    #[schemars(description = "Optional control columns for partial correlation computation.")]
    pub control_cols: Option<Vec<String>>,
    #[schemars(description = "Multiple-testing correction method (default 'fdr_bh').")]
    pub correction: Option<String>,
    #[schemars(description = "Optional seeded sampling specification object.")]
    pub sampling: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct StatRegressionFitMcpRequest {
    #[schemars(description = "Dataset input binding object containing dataset_name and parquet_path.")]
    pub input_binding: Value,
    #[schemars(description = "Regression model family (e.g. ols, huber, glm_binomial, glm_poisson).")]
    pub model_type: String,
    #[schemars(description = "Dependent target column name.")]
    pub target_col: String,
    #[schemars(description = "List of predictor/feature column names.")]
    pub feature_cols: Vec<String>,
    #[schemars(description = "Whether to include an intercept term (default true).")]
    pub add_intercept: Option<bool>,
    #[schemars(description = "Optional seeded sampling specification object.")]
    pub sampling: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct StatTimeseriesAnalyzeMcpRequest {
    #[schemars(description = "Dataset input binding object containing dataset_name and parquet_path.")]
    pub input_binding: Value,
    #[schemars(description = "Timestamp or sequential time index column name.")]
    pub time_col: String,
    #[schemars(description = "Primary time-series value column name.")]
    pub value_col: String,
    #[schemars(description = "Optional second series column for spurious cross-series correlation diagnostics.")]
    pub compare_col: Option<String>,
    #[schemars(description = "Optional seasonal period length.")]
    pub period: Option<usize>,
    #[schemars(description = "Optional number of future steps to forecast.")]
    pub forecast_horizon: Option<usize>,
    #[schemars(description = "Forecasting model family (e.g. ets, arima).")]
    pub model: Option<String>,
    #[schemars(description = "Optional z-score threshold for residual anomaly detection.")]
    pub anomaly_z_threshold: Option<f64>,
    #[schemars(description = "Optional seeded sampling specification object.")]
    pub sampling: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct VisualRecommendMcpRequest {
    #[schemars(description = "Registered dataset name in the topology catalog.")]
    pub dataset_name: Option<String>,
    #[schemars(description = "Optional direct Parquet file path to profile and recommend visuals for.")]
    pub parquet_path: Option<String>,
    #[schemars(description = "Preferred chart renderer ('vega_lite' or 'echarts').")]
    pub renderer: Option<String>,
    #[schemars(description = "Maximum number of ranked recommendations to return.")]
    pub max_recommendations: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct VisualRenderSpecMcpRequest {
    #[schemars(description = "Optional human-readable chart title.")]
    pub title: Option<String>,
    #[schemars(description = "Optional chart description.")]
    pub description: Option<String>,
    #[schemars(description = "Chart renderer ('vega_lite' or 'echarts').")]
    pub renderer: Option<String>,
    #[schemars(description = "Chart family identifier (e.g. bar, line, scatter, histogram).")]
    pub chart_family: Option<String>,
    #[schemars(description = "Data binding object specifying dataset, parquet path, SQL, or query plan.")]
    pub data_binding: Value,
    #[schemars(description = "Renderer-specific visualization specification object.")]
    pub spec: Value,
    #[schemars(description = "Optional seeded visual sampling specification object.")]
    pub sampling: Option<Value>,
    #[schemars(description = "Optional analytical question associated with the visual.")]
    pub question: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TopologyGraphMcpRequest {
    #[schemars(description = "Topology projection lens ('semantic_3layer', 'er_join', or 'lineage_overlay').")]
    pub lens: Option<String>,
    #[schemars(description = "Optional source identifier filter.")]
    pub source_id: Option<String>,
    #[schemars(description = "Optional focus node ID for bounded ego-graph traversal.")]
    pub focus_node_id: Option<String>,
    #[schemars(description = "Maximum ego-graph hop depth from focus_node_id.")]
    pub hops: Option<u8>,
    #[schemars(description = "Optional list of dataset IDs to restrict the subgraph.")]
    pub dataset_ids: Option<Vec<String>>,
    #[schemars(description = "Optional epistemic status filters (e.g. ['verified', 'empirically_tested']).")]
    pub epistemic_status: Option<Vec<String>>,
    #[schemars(description = "Whether to include Layer 0 field nodes.")]
    pub include_fields: Option<bool>,
    #[schemars(description = "Whether to include FORBIDDEN_JOIN edges.")]
    pub include_forbidden: Option<bool>,
    #[schemars(description = "Whether to include Layer 2 semantic concept/metric overlays.")]
    pub include_semantics: Option<bool>,
    #[schemars(description = "Optional ProvenanceRun ID for lineage overlay highlighting.")]
    pub provenance_run_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TopologyFieldAnatomyMcpRequest {
    #[schemars(description = "Target anatomy kind: 'dataset' or 'join'.")]
    pub target_kind: String,
    #[schemars(description = "Canonical dataset ID or join hypothesis ID to inspect.")]
    pub target_id: String,
}

// ============================================================================
// DataEngineMcpServer
// ============================================================================

#[derive(Clone)]
pub struct DataEngineMcpServer {
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for DataEngineMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataEngineMcpServer").finish()
    }
}

impl Default for DataEngineMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl DataEngineMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    pub fn list_tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_router.list_all()
    }

    pub fn info(&self, instructions: &str) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(instructions)
    }

    pub async fn call_tool(
        &self,
        state: &AppState,
        name: &str,
        args: Value,
    ) -> Result<Value, McpError> {
        match name {
            "hello" => {
                let who = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("world")
                    .trim();
                Ok(json!({ "message": format!("Hello from data-engine, {}!", who) }))
            }
            "algorithm_browse" => {
                let list = state.algorithm_registry.list()?;
                Ok(json!({ "algorithms": list }))
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
                        VisualsError::InvalidVisualSpec(
                            "missing target_kind ('dataset' or 'join')".to_string(),
                        )
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
}

#[tool_router]
impl DataEngineMcpServer {
    #[tool(description = "Health-check MCP tool returning a greeting from data-engine.")]
    pub fn hello(
        &self,
        Parameters(HelloMcpRequest { name }): Parameters<HelloMcpRequest>,
    ) -> Result<String, String> {
        let who = name.as_deref().unwrap_or("world").trim();
        Ok(format!("Hello from data-engine, {}!", who))
    }

    #[tool(description = "List all hydrated, SHA-256-pinned built-in statistical algorithms.")]
    pub fn algorithm_browse(
        &self,
        _params: Parameters<AlgorithmBrowseMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Compute distributional moments, quantiles, Shapiro-Wilk/D'Agostino normality, outlier counts, and learned field traits."
    )]
    pub fn stat_describe_and_check(
        &self,
        _params: Parameters<StatDescribeMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Execute parametric/non-parametric hypothesis tests with assumption checks, CIs, BH-FDR correction, and Simpson's paradox stratification."
    )]
    pub fn stat_hypothesis_test(
        &self,
        _params: Parameters<StatHypothesisTestMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Compute prospective/retrospective statistical power, required sample size, or MDE."
    )]
    pub fn stat_power_analysis(
        &self,
        _params: Parameters<StatPowerAnalysisMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Compute Pearson/Spearman/Kendall correlation matrices with FDR adjustment, partial correlations, and VIF multicollinearity diagnostics."
    )]
    pub fn stat_correlation_vif(
        &self,
        _params: Parameters<StatCorrelationVifMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Fit OLS, GLM, or Huber robust regression with HC3 SEs, Breusch-Pagan, Durbin-Watson, and Cook's distance diagnostics."
    )]
    pub fn stat_regression_fit(
        &self,
        _params: Parameters<StatRegressionFitMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Perform STL decomposition, ADF/KPSS stationarity, ACF/PACF, spurious cross-series correlation checks, and ETS/ARIMA forecasting."
    )]
    pub fn stat_timeseries_analyze(
        &self,
        _params: Parameters<StatTimeseriesAnalyzeMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Deterministically recommend ranked Vega-Lite v5 or Apache ECharts DataVisualSpec candidates from dataset schema and profile statistics."
    )]
    pub fn visual_recommend(
        &self,
        _params: Parameters<VisualRecommendMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Validate semantic/fan-out guardrails, execute bounded aggregation or seeded sampling with ProvenanceRun tracking, and return a canvas-ready DataVisualSpec for ./DataVisualExplorerWrapper."
    )]
    pub fn visual_render_spec(
        &self,
        _params: Parameters<VisualRenderSpecMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Return a bounded multi-layer SemanticTopologyGraph (Business Concepts/Metrics -> Datasets -> Fields, JOINABLE_WITH, FORBIDDEN_JOIN, fan-out & CONFOUNDED_BY overlays) for ./SemanticTopologyWrapper."
    )]
    pub fn topology_graph(
        &self,
        _params: Parameters<TopologyGraphMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }

    #[tool(
        description = "Return zoomed-in nested Arrow schema anatomy for a dataset or side-by-side schema cards, type compatibility, Venn/Sankey overlap, and fan-out diagnostics for a join hypothesis."
    )]
    pub fn topology_field_anatomy(
        &self,
        _params: Parameters<TopologyFieldAnatomyMcpRequest>,
    ) -> Result<String, String> {
        Ok("Dispatched via call_tool".to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DataEngineMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("PolecatWorks Data-Engine Model Context Protocol Server")
    }
}
