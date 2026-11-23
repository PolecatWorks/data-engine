//! Data Visuals Engine: Schema-Driven Chart Recommender, Bounded Query Materialization,
//! Provenance-Backed Visual Hydration, and Saved Visuals BREAD (Spec 13).

pub mod config;

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, RwLock};

use arrow::array::{Array, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::DataType;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::analytics::{AnalyticsError, FieldStatisticalCatalog};
use crate::profiler::{DatasetProfile, FieldProfile, ProfilerError, profile_parquet_dataset};
use crate::provenance::{ProvenanceError, ProvenanceStore, TrustSummary};
use crate::query_engine::{SemanticQueryPlan, SemanticRuleBinding, validate_query_plan};
use crate::visuals::config::VisualsConfig;

#[derive(Debug, Error)]
pub enum VisualsError {
    #[error("chart query returned {actual_rows} rows which exceeds visuals.max_chart_points ({max_chart_points}); aggregate with GROUP BY/binning or supply explicit seeded sampling")]
    ChartPointLimitExceeded {
        actual_rows: usize,
        max_chart_points: usize,
    },

    #[error("invalid visual specification: {0}")]
    InvalidVisualSpec(String),

    #[error("unsupported visual renderer '{0}' (expected 'vega_lite' or 'echarts')")]
    UnsupportedRenderer(String),

    #[error("saved visual '{0}' not found")]
    VisualNotFound(Uuid),

    #[error("topology focus node or resource '{0}' not found")]
    TopologyFocusNotFound(String),

    #[error("query blocked by forbidden join rule(s): {0}")]
    ForbiddenJoinBlocked(String),

    #[error("query blocked by un-aggregated fan-out trap: {0}")]
    FanOutTrapBlocked(String),

    #[error("dataset or parquet binding required: {0}")]
    InvalidBinding(String),

    #[error("visuals store lock poisoned")]
    LockPoisoned,

    #[error("GraphRAG overlay error: {0}")]
    GraphragOverlay(String),

    #[error(transparent)]
    Analytics(#[from] AnalyticsError),

    #[error(transparent)]
    Profiler(#[from] ProfilerError),

    #[error(transparent)]
    Provenance(#[from] ProvenanceError),

    #[error("I/O error in visuals engine: {0}")]
    Io(#[from] std::io::Error),

    #[error("parquet error in visuals engine: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("arrow error in visuals engine: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualSamplingSpec {
    #[serde(default = "default_sampling_strategy")]
    pub strategy: String,
    pub max_rows: usize,
    pub seed: u64,
    #[serde(default)]
    pub stratify_by: Option<String>,
}

fn default_sampling_strategy() -> String {
    "random".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataBinding {
    /// One of: "dataset" | "parquet" | "sql_query" | "algorithm"
    #[serde(default = "default_binding_kind")]
    pub kind: String,
    #[serde(default)]
    pub dataset_id: Option<String>,
    #[serde(default)]
    pub parquet_path: Option<String>,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub algorithm_id: Option<String>,
    #[serde(default)]
    pub group_by: Option<String>,
    #[serde(default)]
    pub metric_column: Option<String>,
    #[serde(default)]
    pub aggregation: Option<String>,
    #[serde(default)]
    pub semantic_rules: Vec<SemanticRuleBinding>,
    #[serde(default)]
    pub query_plan: Option<SemanticQueryPlan>,
}

fn default_binding_kind() -> String {
    "parquet".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReferenceLine {
    pub axis: String,
    pub value: f64,
    pub label: String,
    #[serde(default = "default_dashed")]
    pub style: String,
}

fn default_dashed() -> String {
    "dashed".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualAnnotation {
    pub x: Value,
    pub label: String,
    #[serde(default = "default_info")]
    pub severity: String,
}

fn default_info() -> String {
    "info".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProfileOverlays {
    #[serde(default)]
    pub reference_lines: Vec<ReferenceLine>,
    #[serde(default)]
    pub annotations: Vec<VisualAnnotation>,
    #[serde(default)]
    pub log_scale_recommended: bool,
    #[serde(default)]
    pub seasonality_period: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualProvenance {
    pub provenance_run_id: String,
    pub trust_summary: TrustSummary,
    pub sampled: bool,
    pub population_rows: usize,
    pub rendered_rows: usize,
    pub sampling_seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataVisualSpec {
    pub visual_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_renderer")]
    pub renderer: String,
    pub chart_family: String,
    pub data_binding: DataBinding,
    pub spec: Value,
    #[serde(default)]
    pub profile_overlays: ProfileOverlays,
}

fn default_renderer() -> String {
    "vega_lite".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChartRecommendation {
    pub rank: usize,
    pub score: f64,
    pub rationale: String,
    pub visual_spec: DataVisualSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualRecommendRequest {
    #[serde(default)]
    pub dataset_name: Option<String>,
    #[serde(default)]
    pub parquet_path: Option<String>,
    #[serde(default)]
    pub profile: Option<DatasetProfile>,
    #[serde(default)]
    pub renderer: Option<String>,
    #[serde(default)]
    pub max_recommendations: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualRecommendResponse {
    pub dataset_name: String,
    pub recommendations: Vec<ChartRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualQueryRequest {
    #[serde(default = "default_visual_title")]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_renderer")]
    pub renderer: String,
    #[serde(default = "default_chart_family")]
    pub chart_family: String,
    pub data_binding: DataBinding,
    pub spec: Value,
    #[serde(default)]
    pub profile_overlays: ProfileOverlays,
    #[serde(default)]
    pub sampling: Option<VisualSamplingSpec>,
    #[serde(default)]
    pub question: Option<String>,
}

fn default_visual_title() -> String {
    "Data Visual".to_string()
}

fn default_chart_family() -> String {
    "bar".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualQueryResponse {
    pub visual_spec: DataVisualSpec,
    pub data_values: Vec<Value>,
    pub provenance: VisualProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedVisualRecord {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub renderer: String,
    pub chart_family: String,
    pub primary_dataset_id: Option<String>,
    pub data_binding: DataBinding,
    pub visual_spec: Value,
    pub profile_overlays: ProfileOverlays,
    pub last_provenance_run_id: Option<String>,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedVisualCreateRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_renderer")]
    pub renderer: String,
    pub chart_family: String,
    #[serde(default)]
    pub primary_dataset_id: Option<String>,
    pub data_binding: DataBinding,
    pub visual_spec: Value,
    #[serde(default)]
    pub profile_overlays: ProfileOverlays,
    #[serde(default)]
    pub last_provenance_run_id: Option<String>,
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

fn default_created_by() -> String {
    "system".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedVisualUpdateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub renderer: Option<String>,
    #[serde(default)]
    pub chart_family: Option<String>,
    #[serde(default)]
    pub data_binding: Option<DataBinding>,
    #[serde(default)]
    pub visual_spec: Option<Value>,
    #[serde(default)]
    pub profile_overlays: Option<ProfileOverlays>,
    #[serde(default)]
    pub last_provenance_run_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct SavedVisualStore {
    items: Arc<RwLock<BTreeMap<Uuid, SavedVisualRecord>>>,
}

impl SavedVisualStore {
    pub fn list(
        &self,
        dataset_id: Option<&str>,
        chart_family: Option<&str>,
        q: Option<&str>,
    ) -> Result<Vec<SavedVisualRecord>, VisualsError> {
        let guard = self.items.read().map_err(|_| VisualsError::LockPoisoned)?;
        let q_lower = q.map(|s| s.to_ascii_lowercase());
        Ok(guard
            .values()
            .filter(|rec| {
                if let Some(ds) = dataset_id
                    && rec.primary_dataset_id.as_deref() != Some(ds)
                {
                    return false;
                }
                if let Some(fam) = chart_family
                    && rec.chart_family != fam
                {
                    return false;
                }
                if let Some(ref query) = q_lower
                    && !rec.name.to_ascii_lowercase().contains(query)
                    && !rec.description.to_ascii_lowercase().contains(query)
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect())
    }

    pub fn get(&self, id: Uuid) -> Result<SavedVisualRecord, VisualsError> {
        let guard = self.items.read().map_err(|_| VisualsError::LockPoisoned)?;
        guard
            .get(&id)
            .cloned()
            .ok_or(VisualsError::VisualNotFound(id))
    }

    pub fn create(&self, req: SavedVisualCreateRequest) -> Result<SavedVisualRecord, VisualsError> {
        validate_renderer_and_spec(&req.renderer, &req.visual_spec)?;
        if req.name.trim().is_empty() {
            return Err(VisualsError::InvalidVisualSpec(
                "name must not be empty".to_string(),
            ));
        }
        let id = Uuid::new_v4();
        let record = SavedVisualRecord {
            id,
            name: req.name,
            description: req.description,
            renderer: req.renderer,
            chart_family: req.chart_family,
            primary_dataset_id: req.primary_dataset_id,
            data_binding: req.data_binding,
            visual_spec: req.visual_spec,
            profile_overlays: req.profile_overlays,
            last_provenance_run_id: req.last_provenance_run_id,
            created_by: req.created_by,
        };
        let mut guard = self.items.write().map_err(|_| VisualsError::LockPoisoned)?;
        guard.insert(id, record.clone());
        Ok(record)
    }

    pub fn update(
        &self,
        id: Uuid,
        req: SavedVisualUpdateRequest,
    ) -> Result<SavedVisualRecord, VisualsError> {
        let mut guard = self.items.write().map_err(|_| VisualsError::LockPoisoned)?;
        let existing = guard
            .get_mut(&id)
            .ok_or(VisualsError::VisualNotFound(id))?;
        let next_renderer = req.renderer.as_deref().unwrap_or(&existing.renderer);
        let next_spec = req.visual_spec.as_ref().unwrap_or(&existing.visual_spec);
        validate_renderer_and_spec(next_renderer, next_spec)?;

        if let Some(name) = req.name {
            if name.trim().is_empty() {
                return Err(VisualsError::InvalidVisualSpec(
                    "name must not be empty".to_string(),
                ));
            }
            existing.name = name;
        }
        if let Some(desc) = req.description {
            existing.description = desc;
        }
        if let Some(r) = req.renderer {
            existing.renderer = r;
        }
        if let Some(fam) = req.chart_family {
            existing.chart_family = fam;
        }
        if let Some(binding) = req.data_binding {
            existing.data_binding = binding;
        }
        if let Some(spec) = req.visual_spec {
            existing.visual_spec = spec;
        }
        if let Some(overlays) = req.profile_overlays {
            existing.profile_overlays = overlays;
        }
        if let Some(prov) = req.last_provenance_run_id {
            existing.last_provenance_run_id = Some(prov);
        }
        Ok(existing.clone())
    }

    pub fn delete(&self, id: Uuid) -> Result<(), VisualsError> {
        let mut guard = self.items.write().map_err(|_| VisualsError::LockPoisoned)?;
        guard
            .remove(&id)
            .map(|_| ())
            .ok_or(VisualsError::VisualNotFound(id))
    }
}

pub fn validate_renderer_and_spec(renderer: &str, spec: &Value) -> Result<(), VisualsError> {
    if renderer != "vega_lite" && renderer != "echarts" {
        return Err(VisualsError::UnsupportedRenderer(renderer.to_string()));
    }
    let Some(obj) = spec.as_object() else {
        return Err(VisualsError::InvalidVisualSpec(
            "spec must be a JSON object".to_string(),
        ));
    };
    if renderer == "vega_lite" {
        let has_mark = obj.contains_key("mark")
            || obj.contains_key("layer")
            || obj.contains_key("vconcat")
            || obj.contains_key("hconcat");
        if !has_mark {
            return Err(VisualsError::InvalidVisualSpec(
                "vega_lite spec must define 'mark', 'layer', 'vconcat', or 'hconcat'".to_string(),
            ));
        }
    } else if !obj.contains_key("series") {
        return Err(VisualsError::InvalidVisualSpec(
            "echarts spec must define 'series'".to_string(),
        ));
    }
    Ok(())
}

fn is_temporal_field(f: &FieldProfile) -> bool {
    let dt = f.arrow_data_type.to_ascii_lowercase();
    let name = f.field_name.to_ascii_lowercase();
    dt.contains("date")
        || dt.contains("timestamp")
        || f.inferred_semantic_domain == "temporal"
        || name == "day"
        || name == "date"
        || name.ends_with("_date")
        || name.ends_with("_at")
        || name.ends_with("_ts")
}

fn is_numeric_measure(f: &FieldProfile) -> bool {
    if f.numeric_stats.is_none() {
        return false;
    }
    let name = f.field_name.to_ascii_lowercase();
    let is_id_like = name == "id" || name.ends_with("_id") || f.inferred_semantic_domain == "identifier";
    !is_id_like
}

fn is_low_cardinality_categorical(f: &FieldProfile) -> bool {
    let dt = f.arrow_data_type.to_ascii_lowercase();
    (dt.contains("utf8") || dt.contains("bool") || f.inferred_semantic_domain == "categorical")
        && f.distinct_count >= 2
        && f.distinct_count <= 20
        && !is_temporal_field(f)
}

fn build_spec_for_renderer(renderer: &str, vega_spec: Value, chart_family: &str, x_field: &str, y_field: Option<&str>) -> Value {
    if renderer == "echarts" {
        let echarts_type = match chart_family {
            "line" | "area" => "line",
            "scatter" | "bubble" => "scatter",
            "box_plot" => "boxplot",
            "heatmap" => "heatmap",
            _ => "bar",
        };
        json!({
            "tooltip": { "trigger": "axis" },
            "xAxis": { "type": "category", "name": x_field },
            "yAxis": { "type": "value", "name": y_field.unwrap_or("count") },
            "series": [{
                "name": y_field.unwrap_or(x_field),
                "type": echarts_type,
                "encode": { "x": x_field, "y": y_field.unwrap_or("count") }
            }]
        })
    } else {
        vega_spec
    }
}

/// Deterministically recommends ranked Vega-Lite v5 / ECharts `DataVisualSpec` candidates
/// from Arrow schema types, column profile statistics, and learned statistical traits.
pub fn recommend_visuals(
    profile: &DatasetProfile,
    parquet_path: Option<&str>,
    field_catalog: &FieldStatisticalCatalog,
    config: &VisualsConfig,
    renderer_override: Option<&str>,
) -> Result<VisualRecommendResponse, VisualsError> {
    let renderer = renderer_override.unwrap_or("vega_lite");
    if renderer != "vega_lite" && renderer != "echarts" {
        return Err(VisualsError::UnsupportedRenderer(renderer.to_string()));
    }

    let temporals: Vec<&FieldProfile> = profile.fields.iter().filter(|f| is_temporal_field(f)).collect();
    let numerics: Vec<&FieldProfile> = profile.fields.iter().filter(|f| is_numeric_measure(f)).collect();
    let categoricals: Vec<&FieldProfile> = profile
        .fields
        .iter()
        .filter(|f| is_low_cardinality_categorical(f))
        .collect();

    let mut candidates: Vec<(f64, String, DataVisualSpec)> = Vec::new();
    let base_binding = |group_by: Option<String>, metric: Option<String>, agg: Option<String>, sql: String| DataBinding {
        kind: if parquet_path.is_some() {
            "parquet".to_string()
        } else {
            "dataset".to_string()
        },
        dataset_id: Some(profile.dataset_name.clone()),
        parquet_path: parquet_path.map(str::to_string),
        sql: Some(sql),
        algorithm_id: None,
        group_by,
        metric_column: metric,
        aggregation: agg,
        semantic_rules: vec![],
        query_plan: None,
    };

    // 1. Temporal + Numeric -> Line & Area
    if let (Some(t), Some(num)) = (temporals.first(), numerics.first()) {
        let canonical_num = format!("{}.{}", profile.dataset_name, num.field_name);
        let trait_rec = field_catalog.get(&canonical_num)?;
        let mut overlays = ProfileOverlays::default();
        if let Some(ref tr) = trait_rec {
            overlays.seasonality_period = tr.seasonality_period.clone();
            if tr.is_heavy_tailed {
                overlays.log_scale_recommended = true;
            }
        }
        if let Some(ref ns) = num.numeric_stats {
            overlays.reference_lines.push(ReferenceLine {
                axis: "y".to_string(),
                value: ns.mean,
                label: format!("Mean {}", num.field_name),
                style: "dashed".to_string(),
            });
        }
        let vega_line = json!({
            "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
            "mark": { "type": "line", "point": true },
            "encoding": {
                "x": { "field": t.field_name, "type": "temporal", "title": t.field_name },
                "y": { "field": num.field_name, "type": "quantitative", "title": num.field_name }
            }
        });
        let sql = format!(
            "SELECT {}, SUM({}) AS {} FROM {} GROUP BY 1 ORDER BY 1",
            t.field_name, num.field_name, num.field_name, profile.dataset_name
        );
        candidates.push((
            0.95,
            format!(
                "Temporal column '{}' paired with quantitative measure '{}' is best viewed as a time-series line chart.",
                t.field_name, num.field_name
            ),
            DataVisualSpec {
                visual_id: format!("rec-line-{}-{}", t.field_name, num.field_name),
                title: format!("{} over {}", num.field_name, t.field_name),
                description: "Auto-recommended temporal trend".to_string(),
                renderer: renderer.to_string(),
                chart_family: "line".to_string(),
                data_binding: base_binding(
                    Some(t.field_name.clone()),
                    Some(num.field_name.clone()),
                    Some("sum".to_string()),
                    sql,
                ),
                spec: build_spec_for_renderer(renderer, vega_line, "line", &t.field_name, Some(&num.field_name)),
                profile_overlays: overlays,
            },
        ));
    }

    // 2. Low-Cardinality Categorical + Numeric -> Heavy-tail promotes Box Plot above Bar
    if let (Some(cat), Some(num)) = (categoricals.first(), numerics.first()) {
        let canonical_num = format!("{}.{}", profile.dataset_name, num.field_name);
        let trait_rec = field_catalog.get(&canonical_num)?;
        let is_heavy_tailed = trait_rec
            .as_ref()
            .map(|tr| tr.is_heavy_tailed || tr.skewness.map(|s| s.abs() > 2.0).unwrap_or(false))
            .unwrap_or(false);

        let mut box_overlays = ProfileOverlays {
            log_scale_recommended: is_heavy_tailed,
            ..Default::default()
        };
        if let Some(ref ns) = num.numeric_stats {
            box_overlays.reference_lines.push(ReferenceLine {
                axis: "y".to_string(),
                value: ns.mean,
                label: if is_heavy_tailed {
                    format!("Median / Reference ({})", num.field_name)
                } else {
                    format!("Mean ({})", num.field_name)
                },
                style: "dashed".to_string(),
            });
        }

        let y_encoding = if is_heavy_tailed {
            json!({
                "field": num.field_name,
                "type": "quantitative",
                "scale": { "type": "log" },
                "title": format!("{} (log scale)", num.field_name)
            })
        } else {
            json!({
                "field": num.field_name,
                "type": "quantitative",
                "title": num.field_name
            })
        };

        let vega_box = json!({
            "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
            "mark": { "type": "boxplot", "extent": "min-max" },
            "encoding": {
                "x": { "field": cat.field_name, "type": "nominal", "title": cat.field_name },
                "y": y_encoding
            }
        });
        let box_score = if is_heavy_tailed { 0.94 } else { 0.80 };
        let box_rationale = if is_heavy_tailed {
            format!(
                "Field '{}' is heavy-tailed; box plot with log-scale hint and reference line exposes outliers better than a mean bar chart.",
                num.field_name
            )
        } else {
            format!(
                "Compare distribution spread of '{}' across '{}' categories.",
                num.field_name, cat.field_name
            )
        };
        candidates.push((
            box_score,
            box_rationale,
            DataVisualSpec {
                visual_id: format!("rec-box-{}-{}", cat.field_name, num.field_name),
                title: format!("{} distribution by {}", num.field_name, cat.field_name),
                description: "Auto-recommended categorical spread comparison".to_string(),
                renderer: renderer.to_string(),
                chart_family: "box_plot".to_string(),
                data_binding: base_binding(
                    None,
                    Some(num.field_name.clone()),
                    None,
                    format!(
                        "SELECT {}, {} FROM {}",
                        cat.field_name, num.field_name, profile.dataset_name
                    ),
                ),
                spec: build_spec_for_renderer(renderer, vega_box, "box_plot", &cat.field_name, Some(&num.field_name)),
                profile_overlays: box_overlays,
            },
        ));

        let vega_bar = json!({
            "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
            "mark": "bar",
            "encoding": {
                "x": { "field": cat.field_name, "type": "nominal", "sort": "-y", "title": cat.field_name },
                "y": { "field": num.field_name, "type": "quantitative", "aggregate": "sum", "title": format!("Total {}", num.field_name) }
            }
        });
        let bar_score = if is_heavy_tailed { 0.78 } else { 0.92 };
        candidates.push((
            bar_score,
            format!(
                "Low-cardinality category '{}' ({} distinct) paired with '{}' supports ranked bar comparison.",
                cat.field_name, cat.distinct_count, num.field_name
            ),
            DataVisualSpec {
                visual_id: format!("rec-bar-{}-{}", cat.field_name, num.field_name),
                title: format!("Total {} by {}", num.field_name, cat.field_name),
                description: "Auto-recommended ranked categorical bar chart".to_string(),
                renderer: renderer.to_string(),
                chart_family: "bar".to_string(),
                data_binding: base_binding(
                    Some(cat.field_name.clone()),
                    Some(num.field_name.clone()),
                    Some("sum".to_string()),
                    format!(
                        "SELECT {}, SUM({}) AS {} FROM {} GROUP BY 1 ORDER BY 2 DESC",
                        cat.field_name, num.field_name, num.field_name, profile.dataset_name
                    ),
                ),
                spec: build_spec_for_renderer(renderer, vega_bar, "bar", &cat.field_name, Some(&num.field_name)),
                profile_overlays: ProfileOverlays::default(),
            },
        ));
    }

    // 3. Two Numerics -> Scatter or 2D Binned Heatmap
    if numerics.len() >= 2 {
        let x = numerics[0];
        let y = numerics[1];
        let use_binned_heatmap = profile.row_count > config.max_chart_points;
        let family = if use_binned_heatmap { "heatmap" } else { "scatter" };
        let vega_bi = if use_binned_heatmap {
            json!({
                "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
                "mark": "rect",
                "encoding": {
                    "x": { "field": x.field_name, "bin": { "maxbins": 30 }, "type": "quantitative" },
                    "y": { "field": y.field_name, "bin": { "maxbins": 30 }, "type": "quantitative" },
                    "color": { "aggregate": "count", "type": "quantitative" }
                }
            })
        } else {
            json!({
                "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
                "mark": { "type": "point", "filled": true, "opacity": 0.7 },
                "encoding": {
                    "x": { "field": x.field_name, "type": "quantitative" },
                    "y": { "field": y.field_name, "type": "quantitative" }
                }
            })
        };
        candidates.push((
            0.88,
            format!(
                "Two quantitative columns '{}' and '{}' reveal bivariate correlation and outliers.",
                x.field_name, y.field_name
            ),
            DataVisualSpec {
                visual_id: format!("rec-{}-{}-{}", family, x.field_name, y.field_name),
                title: format!("{} vs {}", y.field_name, x.field_name),
                description: "Auto-recommended bivariate relationship plot".to_string(),
                renderer: renderer.to_string(),
                chart_family: family.to_string(),
                data_binding: base_binding(
                    None,
                    Some(y.field_name.clone()),
                    None,
                    format!("SELECT {}, {} FROM {}", x.field_name, y.field_name, profile.dataset_name),
                ),
                spec: build_spec_for_renderer(renderer, vega_bi, family, &x.field_name, Some(&y.field_name)),
                profile_overlays: ProfileOverlays::default(),
            },
        ));
    }

    // 4. Two Categoricals + Numeric -> Heatmap
    if categoricals.len() >= 2
        && let Some(num) = numerics.first()
    {
        let c1 = categoricals[0];
        let c2 = categoricals[1];
        let vega_hm = json!({
            "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
            "mark": "rect",
            "encoding": {
                "x": { "field": c1.field_name, "type": "nominal" },
                "y": { "field": c2.field_name, "type": "nominal" },
                "color": { "field": num.field_name, "aggregate": "sum", "type": "quantitative" }
            }
        });
        candidates.push((
            0.86,
            format!(
                "Two categorical dimensions ('{}', '{}') cross-tabulated by '{}' fit a matrix heatmap.",
                c1.field_name, c2.field_name, num.field_name
            ),
            DataVisualSpec {
                visual_id: format!("rec-heatmap-{}-{}", c1.field_name, c2.field_name),
                title: format!("{} by {} x {}", num.field_name, c1.field_name, c2.field_name),
                description: "Auto-recommended categorical heatmap".to_string(),
                renderer: renderer.to_string(),
                chart_family: "heatmap".to_string(),
                data_binding: base_binding(
                    Some(c1.field_name.clone()),
                    Some(num.field_name.clone()),
                    Some("sum".to_string()),
                    format!(
                        "SELECT {}, {}, SUM({}) AS {} FROM {} GROUP BY 1, 2",
                        c1.field_name, c2.field_name, num.field_name, num.field_name, profile.dataset_name
                    ),
                ),
                spec: build_spec_for_renderer(renderer, vega_hm, "heatmap", &c1.field_name, Some(&c2.field_name)),
                profile_overlays: ProfileOverlays::default(),
            },
        ));
    }

    // 5. Single Numeric -> Distribution Histogram
    if let Some(num) = numerics.first() {
        let mut overlays = ProfileOverlays::default();
        if let Some(ref ns) = num.numeric_stats {
            overlays.reference_lines.push(ReferenceLine {
                axis: "x".to_string(),
                value: ns.mean,
                label: format!("Mean ({:.2})", ns.mean),
                style: "dashed".to_string(),
            });
        }
        let vega_hist = json!({
            "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
            "mark": "bar",
            "encoding": {
                "x": { "field": num.field_name, "bin": true, "type": "quantitative" },
                "y": { "aggregate": "count", "type": "quantitative" }
            }
        });
        candidates.push((
            0.85,
            format!("Histogram reveals distribution shape and skewness of '{}'.", num.field_name),
            DataVisualSpec {
                visual_id: format!("rec-hist-{}", num.field_name),
                title: format!("Distribution of {}", num.field_name),
                description: "Auto-recommended univariate distribution histogram".to_string(),
                renderer: renderer.to_string(),
                chart_family: "histogram".to_string(),
                data_binding: base_binding(
                    None,
                    Some(num.field_name.clone()),
                    None,
                    format!("SELECT {} FROM {}", num.field_name, profile.dataset_name),
                ),
                spec: build_spec_for_renderer(renderer, vega_hist, "histogram", &num.field_name, None),
                profile_overlays: overlays,
            },
        ));
    }

    // 6. Single Categorical -> Top-K Bar
    if let Some(cat) = categoricals.first() {
        let vega_topk = json!({
            "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
            "mark": "bar",
            "encoding": {
                "x": { "field": cat.field_name, "type": "nominal", "sort": "-y" },
                "y": { "aggregate": "count", "type": "quantitative" }
            }
        });
        candidates.push((
            0.84,
            format!("Frequency bar chart for categorical column '{}'.", cat.field_name),
            DataVisualSpec {
                visual_id: format!("rec-topk-{}", cat.field_name),
                title: format!("Count by {}", cat.field_name),
                description: "Auto-recommended categorical frequency chart".to_string(),
                renderer: renderer.to_string(),
                chart_family: "bar".to_string(),
                data_binding: base_binding(
                    Some(cat.field_name.clone()),
                    None,
                    Some("count".to_string()),
                    format!("SELECT {}, COUNT(*) AS count FROM {} GROUP BY 1 ORDER BY 2 DESC", cat.field_name, profile.dataset_name),
                ),
                spec: build_spec_for_renderer(renderer, vega_topk, "bar", &cat.field_name, None),
                profile_overlays: ProfileOverlays::default(),
            },
        ));
    }

    // Sort deterministically by (score DESC, visual_id ASC)
    candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.2.visual_id.cmp(&b.2.visual_id))
    });

    let recommendations = candidates
        .into_iter()
        .enumerate()
        .map(|(idx, (score, rationale, visual_spec))| ChartRecommendation {
            rank: idx + 1,
            score,
            rationale,
            visual_spec,
        })
        .collect();

    Ok(VisualRecommendResponse {
        dataset_name: profile.dataset_name.clone(),
        recommendations,
    })
}

fn deterministic_row_hash(row_idx: usize, seed: u64) -> u64 {
    let mut x = (row_idx as u64).wrapping_add(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn read_parquet_rows_as_json(parquet_path: &Path) -> Result<Vec<serde_json::Map<String, Value>>, VisualsError> {
    let file = File::open(parquet_path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;
    let schema = builder.schema().clone();
    let reader = builder.build()?;
    let mut rows = Vec::new();

    for batch_res in reader {
        let batch = batch_res?;
        for row_idx in 0..batch.num_rows() {
            let mut obj = serde_json::Map::new();
            for (col_idx, field) in schema.fields().iter().enumerate() {
                let col = batch.column(col_idx);
                let val = if col.is_null(row_idx) {
                    Value::Null
                } else {
                    match field.data_type() {
                        DataType::Utf8 => col
                            .as_any()
                            .downcast_ref::<StringArray>()
                            .map(|a| Value::String(a.value(row_idx).to_string()))
                            .unwrap_or(Value::Null),
                        DataType::Int64 => col
                            .as_any()
                            .downcast_ref::<Int64Array>()
                            .map(|a| json!(a.value(row_idx)))
                            .unwrap_or(Value::Null),
                        DataType::Float64 => col
                            .as_any()
                            .downcast_ref::<Float64Array>()
                            .map(|a| json!(a.value(row_idx)))
                            .unwrap_or(Value::Null),
                        DataType::Boolean => col
                            .as_any()
                            .downcast_ref::<BooleanArray>()
                            .map(|a| Value::Bool(a.value(row_idx)))
                            .unwrap_or(Value::Null),
                        _ => Value::String(
                            arrow::util::display::array_value_to_string(col, row_idx)
                                .unwrap_or_default(),
                        ),
                    }
                };
                obj.insert(field.name().clone(), val);
            }
            rows.push(obj);
        }
    }
    Ok(rows)
}

fn aggregate_rows(
    rows: &[serde_json::Map<String, Value>],
    group_by: &str,
    metric_col: Option<&str>,
    aggregation: &str,
) -> Vec<serde_json::Map<String, Value>> {
    let mut buckets: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for row in rows {
        let key = match row.get(group_by) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => "null".to_string(),
            Some(other) => other.to_string(),
        };
        let entry = buckets.entry(key).or_default();
        if let Some(m_col) = metric_col {
            if let Some(v) = row.get(m_col).and_then(|v| v.as_f64()) {
                entry.push(v);
            }
        } else {
            entry.push(1.0);
        }
    }

    let out_col = metric_col.unwrap_or("count");
    let agg_lower = aggregation.to_ascii_lowercase();
    buckets
        .into_iter()
        .map(|(k, vals)| {
            let agg_val = match agg_lower.as_str() {
                "avg" | "mean" => {
                    if vals.is_empty() {
                        0.0
                    } else {
                        vals.iter().sum::<f64>() / (vals.len() as f64)
                    }
                }
                "min" => vals.iter().copied().fold(f64::INFINITY, f64::min),
                "max" => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                "count" => vals.len() as f64,
                _ => vals.iter().sum::<f64>(),
            };
            let mut m = serde_json::Map::new();
            m.insert(group_by.to_string(), Value::String(k));
            m.insert(out_col.to_string(), json!(agg_val));
            m
        })
        .collect()
}

/// Validates semantic/fan-out guardrails, enforces fail-fast `max_chart_points` (or explicit seeded sampling),
/// records an immutable `ProvenanceRun`, and returns a canvas-ready `VisualQueryResponse`.
pub fn execute_visual_query(
    req: &VisualQueryRequest,
    resolved_parquet_path: Option<&Path>,
    config: &VisualsConfig,
    provenance_store: &ProvenanceStore,
) -> Result<VisualQueryResponse, VisualsError> {
    validate_renderer_and_spec(&req.renderer, &req.spec)?;

    // 1. Validate SemanticQueryPlan or inline SemanticRuleBindings (blocks FORBIDDEN_JOIN & fan-out traps)
    let mut rules_applied: Vec<String> = Vec::new();
    let mut all_verified = true;
    let mut min_conf = 1.0_f64;
    let mut stewards: Vec<String> = Vec::new();
    let mut pre_agg = false;

    if let Some(ref plan) = req.data_binding.query_plan {
        let validation = validate_query_plan(plan);
        if !validation.blocked_forbidden_joins.is_empty() {
            return Err(VisualsError::ForbiddenJoinBlocked(
                validation.blocked_forbidden_joins.join("; "),
            ));
        }
        if !validation.fan_out_warnings.is_empty() && req.data_binding.group_by.is_none() {
            return Err(VisualsError::FanOutTrapBlocked(
                validation.fan_out_warnings.join("; "),
            ));
        }
        pre_agg = validation.pre_aggregation_rewrite_required;
        for r in &plan.semantic_rules {
            rules_applied.push(r.rule_id.clone());
            min_conf = min_conf.min(r.confidence);
            if r.epistemic_status != "verified" {
                all_verified = false;
            }
            if let Some(ref st) = r.verified_by
                && !stewards.contains(st)
            {
                stewards.push(st.clone());
            }
        }
    } else if !req.data_binding.semantic_rules.is_empty() {
        let synthetic_plan = SemanticQueryPlan {
            query_name: req.title.clone(),
            left_dataset: req
                .data_binding
                .dataset_id
                .clone()
                .unwrap_or_else(|| "visual_dataset".to_string()),
            left_parquet: req
                .data_binding
                .parquet_path
                .clone()
                .unwrap_or_default(),
            left_join_field: None,
            right_dataset: None,
            right_parquet: None,
            right_join_field: None,
            metric_dataset: req
                .data_binding
                .dataset_id
                .clone()
                .unwrap_or_else(|| "visual_dataset".to_string()),
            metric_field: req
                .data_binding
                .metric_column
                .clone()
                .unwrap_or_else(|| "value".to_string()),
            aggregation: req
                .data_binding
                .aggregation
                .clone()
                .unwrap_or_else(|| "SUM".to_string()),
            semantic_rules: req.data_binding.semantic_rules.clone(),
            enforce_epistemic_gate: true,
        };
        let validation = validate_query_plan(&synthetic_plan);
        if !validation.blocked_forbidden_joins.is_empty() {
            return Err(VisualsError::ForbiddenJoinBlocked(
                validation.blocked_forbidden_joins.join("; "),
            ));
        }
        if !validation.fan_out_warnings.is_empty() && req.data_binding.group_by.is_none() {
            return Err(VisualsError::FanOutTrapBlocked(
                validation.fan_out_warnings.join("; "),
            ));
        }
        for r in &req.data_binding.semantic_rules {
            rules_applied.push(r.rule_id.clone());
            min_conf = min_conf.min(r.confidence);
            if r.epistemic_status != "verified" {
                all_verified = false;
            }
            if let Some(ref st) = r.verified_by
                && !stewards.contains(st)
            {
                stewards.push(st.clone());
            }
        }
    }

    // Also inspect raw SQL string for destructive statements (read-only guard)
    if let Some(ref sql) = req.data_binding.sql {
        let upper = sql.trim().to_ascii_uppercase();
        for forbidden in ["INSERT ", "UPDATE ", "DELETE ", "DROP ", "ALTER ", "TRUNCATE "] {
            if upper.starts_with(forbidden) || upper.contains(&format!("; {}", forbidden)) {
                return Err(VisualsError::InvalidVisualSpec(format!(
                    "read-only SQL required; forbidden statement detected ({})",
                    forbidden.trim()
                )));
            }
        }
    }

    // 2. Materialize bounded data from Parquet
    let parquet_path = resolved_parquet_path
        .map(Path::to_path_buf)
        .or_else(|| req.data_binding.parquet_path.as_deref().map(std::path::PathBuf::from))
        .ok_or_else(|| {
            VisualsError::InvalidBinding(
                "provide data_binding.parquet_path or a catalog-registered dataset_id".to_string(),
            )
        })?;

    let raw_rows = read_parquet_rows_as_json(&parquet_path)?;
    let evaluated_rows = if let Some(ref gb) = req.data_binding.group_by {
        let agg = req.data_binding.aggregation.as_deref().unwrap_or("sum");
        aggregate_rows(
            &raw_rows,
            gb,
            req.data_binding.metric_column.as_deref(),
            agg,
        )
    } else {
        raw_rows
    };

    let population_rows = evaluated_rows.len();
    let (final_rows, sampled, sampling_seed) = if let Some(ref samp) = req.sampling {
        let cap = samp.max_rows.min(config.max_chart_points);
        if cap == 0 {
            return Err(VisualsError::InvalidVisualSpec(
                "sampling.max_rows must be greater than 0".to_string(),
            ));
        }
        if population_rows > cap {
            let mut indexed: Vec<(u64, usize)> = (0..population_rows)
                .map(|i| (deterministic_row_hash(i, samp.seed), i))
                .collect();
            indexed.sort_by_key(|(h, idx)| (*h, *idx));
            let mut chosen: Vec<usize> = indexed.into_iter().take(cap).map(|(_, i)| i).collect();
            chosen.sort_unstable();
            let subset: Vec<Value> = chosen
                .into_iter()
                .map(|i| Value::Object(evaluated_rows[i].clone()))
                .collect();
            (subset, true, Some(samp.seed))
        } else {
            (
                evaluated_rows.into_iter().map(Value::Object).collect(),
                false,
                Some(samp.seed),
            )
        }
    } else if population_rows > config.max_chart_points {
        return Err(VisualsError::ChartPointLimitExceeded {
            actual_rows: population_rows,
            max_chart_points: config.max_chart_points,
        });
    } else {
        (
            evaluated_rows.into_iter().map(Value::Object).collect(),
            false,
            None,
        )
    };

    let rendered_rows = final_rows.len();
    let ds_label = req
        .data_binding
        .dataset_id
        .clone()
        .unwrap_or_else(|| parquet_path.display().to_string());
    let trust_summary = TrustSummary {
        all_rules_human_verified: all_verified,
        min_rule_confidence: min_conf,
        verified_by_stewards: stewards,
        pre_aggregation_applied: pre_agg || req.data_binding.group_by.is_some(),
        unit_conversion_applied: false,
    };

    let run_title = req
        .question
        .as_deref()
        .unwrap_or(&req.title);
    let prov_run = provenance_store.record_run(
        &format!("visual_query:{}", run_title),
        vec![ds_label],
        rules_applied,
        rendered_rows as f64,
        trust_summary.clone(),
    )?;

    let visual_spec = DataVisualSpec {
        visual_id: format!("vis-{}", Uuid::new_v4()),
        title: req.title.clone(),
        description: req.description.clone(),
        renderer: req.renderer.clone(),
        chart_family: req.chart_family.clone(),
        data_binding: req.data_binding.clone(),
        spec: req.spec.clone(),
        profile_overlays: req.profile_overlays.clone(),
    };

    Ok(VisualQueryResponse {
        visual_spec,
        data_values: final_rows,
        provenance: VisualProvenance {
            provenance_run_id: prov_run.provenance_run_id,
            trust_summary,
            sampled,
            population_rows,
            rendered_rows,
            sampling_seed,
        },
    })
}

/// Convenience helper for recommendation when only a parquet path is supplied.
pub fn recommend_visuals_from_parquet(
    dataset_name: &str,
    parquet_path: &Path,
    profiler_cfg: &crate::config::ProfilerConfig,
    field_catalog: &FieldStatisticalCatalog,
    visuals_cfg: &VisualsConfig,
    renderer_override: Option<&str>,
) -> Result<VisualRecommendResponse, VisualsError> {
    let profile = profile_parquet_dataset(dataset_name, parquet_path, profiler_cfg, None)?;
    recommend_visuals(
        &profile,
        Some(&parquet_path.to_string_lossy()),
        field_catalog,
        visuals_cfg,
        renderer_override,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::LearnedFieldTrait;
    use crate::profiler::NumericStats;
    use crate::seeder::seed_validation_suite_with_options;
    use tempfile::tempdir;

    #[test]
    fn test_recommender_ranks_line_for_temporal_and_boxplot_for_heavy_tail() {
        let profile = DatasetProfile {
            dataset_name: "sales_orders".to_string(),
            row_count: 500,
            schema_fingerprint_sha256: "sha1".to_string(),
            candidate_primary_keys: vec!["order_id".to_string()],
            epistemic_status: "verified".to_string(),
            drift_findings: vec![],
            fields: vec![
                FieldProfile {
                    field_name: "order_date".to_string(),
                    arrow_data_type: "Utf8".to_string(),
                    null_count: 0,
                    null_ratio: 0.0,
                    distinct_count: 90,
                    uniqueness_ratio: 0.18,
                    top_k_values: vec!["2026-10-01".to_string()],
                    dominant_prefix: None,
                    minhash_signature: vec![],
                    numeric_stats: None,
                    inferred_semantic_domain: "temporal".to_string(),
                    epistemic_status: "verified".to_string(),
                },
                FieldProfile {
                    field_name: "segment".to_string(),
                    arrow_data_type: "Utf8".to_string(),
                    null_count: 0,
                    null_ratio: 0.0,
                    distinct_count: 3,
                    uniqueness_ratio: 0.006,
                    top_k_values: vec!["enterprise".to_string(), "smb".to_string()],
                    dominant_prefix: None,
                    minhash_signature: vec![],
                    numeric_stats: None,
                    inferred_semantic_domain: "categorical".to_string(),
                    epistemic_status: "verified".to_string(),
                },
                FieldProfile {
                    field_name: "order_total_usd".to_string(),
                    arrow_data_type: "Float64".to_string(),
                    null_count: 0,
                    null_ratio: 0.0,
                    distinct_count: 450,
                    uniqueness_ratio: 0.9,
                    top_k_values: vec![],
                    dominant_prefix: None,
                    minhash_signature: vec![],
                    numeric_stats: Some(NumericStats {
                        min: 10.0,
                        max: 50_000.0,
                        mean: 412.5,
                        is_sequential_integer: false,
                    }),
                    inferred_semantic_domain: "currency".to_string(),
                    epistemic_status: "verified".to_string(),
                },
            ],
        };

        let catalog = FieldStatisticalCatalog::default();
        catalog
            .merge_traits(
                &[LearnedFieldTrait {
                    field_canonical_id: "sales_orders.order_total_usd".to_string(),
                    distribution_family: Some("log_normal_heavy_tailed".to_string()),
                    is_heavy_tailed: true,
                    skewness: Some(4.8),
                    kurtosis: Some(30.2),
                    seasonality_period: Some("7d".to_string()),
                    recommended_test_family: Some("mann_whitney_u".to_string()),
                    known_confounders: vec![],
                    epistemic_status: "empirically_tested".to_string(),
                    confidence: 0.93,
                    verified_by: None,
                    last_diagnosed_run_id: None,
                }],
                "run-test-1",
            )
            .expect("merge trait");

        let resp = recommend_visuals(
            &profile,
            None,
            &catalog,
            &VisualsConfig::default(),
            Some("vega_lite"),
        )
        .expect("recommendations");

        assert!(resp.recommendations.len() >= 3);
        assert_eq!(resp.recommendations[0].visual_spec.chart_family, "line");
        assert_eq!(
            resp.recommendations[0]
                .visual_spec
                .profile_overlays
                .seasonality_period
                .as_deref(),
            Some("7d")
        );

        // Heavy-tailed numeric promotes box_plot above bar with log_scale_recommended + reference line
        let box_pos = resp
            .recommendations
            .iter()
            .position(|r| r.visual_spec.chart_family == "box_plot")
            .expect("box_plot recommended");
        let bar_pos = resp
            .recommendations
            .iter()
            .position(|r| r.visual_spec.chart_family == "bar")
            .expect("bar recommended");
        assert!(box_pos < bar_pos, "heavy-tail should rank box_plot above bar");
        assert!(
            resp.recommendations[box_pos]
                .visual_spec
                .profile_overlays
                .log_scale_recommended
        );
        assert!(
            !resp.recommendations[box_pos]
                .visual_spec
                .profile_overlays
                .reference_lines
                .is_empty()
        );
    }

    #[test]
    fn test_visual_query_blocks_forbidden_join_and_fan_out_trap() {
        let dir = tempdir().unwrap();
        seed_validation_suite_with_options(42, 60, dir.path(), false).unwrap();
        let orders_pq = dir.path().join("sales_orders.parquet");
        let prov = ProvenanceStore::default();
        let cfg = VisualsConfig::default();

        // TRAP-01: FORBIDDEN_JOIN rule blocks chart query before execution
        let forbidden_req = VisualQueryRequest {
            title: "Illegal Homonym Join Chart".to_string(),
            description: String::new(),
            renderer: "vega_lite".to_string(),
            chart_family: "bar".to_string(),
            data_binding: DataBinding {
                kind: "parquet".to_string(),
                dataset_id: Some("billing_subscriptions".to_string()),
                parquet_path: Some(orders_pq.to_string_lossy().to_string()),
                sql: None,
                algorithm_id: None,
                group_by: None,
                metric_column: None,
                aggregation: None,
                semantic_rules: vec![SemanticRuleBinding {
                    rule_id: "trap-01-forbidden".to_string(),
                    rule_type: "FORBIDDEN_JOIN".to_string(),
                    left_dataset: "billing_subscriptions".to_string(),
                    left_field: "account_id".to_string(),
                    right_dataset: Some("marketing_leads".to_string()),
                    right_field: Some("account_id".to_string()),
                    cardinality: "N:M".to_string(),
                    confidence: 1.0,
                    epistemic_status: "verified".to_string(),
                    verified_by: Some("steward_alice".to_string()),
                    scale_divisor: 1.0,
                }],
                query_plan: None,
            },
            spec: json!({ "mark": "bar", "encoding": {} }),
            profile_overlays: ProfileOverlays::default(),
            sampling: None,
            question: None,
        };
        let err = execute_visual_query(&forbidden_req, None, &cfg, &prov)
            .expect_err("FORBIDDEN_JOIN must block visual query");
        assert!(matches!(err, VisualsError::ForbiddenJoinBlocked(_)));

        // TRAP-03: N:M fan-out without group_by pre-aggregation blocks chart query
        let fanout_req = VisualQueryRequest {
            title: "Unaggregated Fan-Out Chart".to_string(),
            description: String::new(),
            renderer: "vega_lite".to_string(),
            chart_family: "bar".to_string(),
            data_binding: DataBinding {
                kind: "parquet".to_string(),
                dataset_id: Some("order_items".to_string()),
                parquet_path: Some(orders_pq.to_string_lossy().to_string()),
                sql: None,
                algorithm_id: None,
                group_by: None,
                metric_column: None,
                aggregation: None,
                semantic_rules: vec![SemanticRuleBinding {
                    rule_id: "trap-03-fanout".to_string(),
                    rule_type: "JOINABLE_WITH".to_string(),
                    left_dataset: "order_items".to_string(),
                    left_field: "order_id".to_string(),
                    right_dataset: Some("order_payments".to_string()),
                    right_field: Some("order_id".to_string()),
                    cardinality: "N:M".to_string(),
                    confidence: 0.95,
                    epistemic_status: "verified".to_string(),
                    verified_by: Some("steward_alice".to_string()),
                    scale_divisor: 1.0,
                }],
                query_plan: None,
            },
            spec: json!({ "mark": "bar", "encoding": {} }),
            profile_overlays: ProfileOverlays::default(),
            sampling: None,
            question: None,
        };
        let err2 = execute_visual_query(&fanout_req, None, &cfg, &prov)
            .expect_err("N:M fan-out without aggregation must block visual query");
        assert!(matches!(err2, VisualsError::FanOutTrapBlocked(_)));
    }

    #[test]
    fn test_visual_query_fails_fast_above_max_chart_points_and_succeeds_with_seeded_sampling() {
        let dir = tempdir().unwrap();
        seed_validation_suite_with_options(42, 200, dir.path(), false).unwrap();
        let orders_pq = dir.path().join("sales_orders.parquet");
        let prov = ProvenanceStore::default();
        let cfg = VisualsConfig {
            max_chart_points: 50,
            ..Default::default()
        };

        let base_req = VisualQueryRequest {
            title: "Orders Scatter".to_string(),
            description: "Raw order amounts".to_string(),
            renderer: "vega_lite".to_string(),
            chart_family: "scatter".to_string(),
            data_binding: DataBinding {
                kind: "parquet".to_string(),
                dataset_id: Some("sales_orders".to_string()),
                parquet_path: Some(orders_pq.to_string_lossy().to_string()),
                sql: None,
                algorithm_id: None,
                group_by: None,
                metric_column: None,
                aggregation: None,
                semantic_rules: vec![],
                query_plan: None,
            },
            spec: json!({ "mark": "point", "encoding": {} }),
            profile_overlays: ProfileOverlays::default(),
            sampling: None,
            question: Some("Plot order amounts".to_string()),
        };

        // Fail fast without explicit sampling (seeder generates 2 * row_count = 400 sales_orders)
        let err = execute_visual_query(&base_req, None, &cfg, &prov)
            .expect_err("unsampled query above max_chart_points must fail fast");
        assert!(matches!(
            err,
            VisualsError::ChartPointLimitExceeded {
                actual_rows: 400,
                max_chart_points: 50
            }
        ));

        // Succeed deterministically with explicit seeded sampling
        let mut sampled_req = base_req;
        sampled_req.sampling = Some(VisualSamplingSpec {
            strategy: "random".to_string(),
            max_rows: 50,
            seed: 42,
            stratify_by: None,
        });
        let resp1 = execute_visual_query(&sampled_req, None, &cfg, &prov).expect("sampled run 1");
        let resp2 = execute_visual_query(&sampled_req, None, &cfg, &prov).expect("sampled run 2");

        assert!(resp1.provenance.sampled);
        assert_eq!(resp1.provenance.population_rows, 400);
        assert_eq!(resp1.provenance.rendered_rows, 50);
        assert_eq!(resp1.provenance.sampling_seed, Some(42));
        assert_eq!(resp1.data_values, resp2.data_values, "seeded sampling must be deterministic");
        assert!(
            prov.get_run(&resp1.provenance.provenance_run_id).is_ok(),
            "ProvenanceRun must be persisted"
        );
    }

    #[test]
    fn test_saved_visuals_bread_lifecycle() {
        let store = SavedVisualStore::default();
        let created = store
            .create(SavedVisualCreateRequest {
                name: "Daily Revenue".to_string(),
                description: "Line trend".to_string(),
                renderer: "vega_lite".to_string(),
                chart_family: "line".to_string(),
                primary_dataset_id: Some("sales_orders".to_string()),
                data_binding: DataBinding {
                    kind: "dataset".to_string(),
                    dataset_id: Some("sales_orders".to_string()),
                    parquet_path: None,
                    sql: None,
                    algorithm_id: None,
                    group_by: None,
                    metric_column: None,
                    aggregation: None,
                    semantic_rules: vec![],
                    query_plan: None,
                },
                visual_spec: json!({ "mark": "line", "encoding": {} }),
                profile_overlays: ProfileOverlays::default(),
                last_provenance_run_id: None,
                created_by: "steward_alice".to_string(),
            })
            .expect("create saved visual");

        assert_eq!(store.list(Some("sales_orders"), None, None).unwrap().len(), 1);
        assert_eq!(store.get(created.id).unwrap().name, "Daily Revenue");

        let updated = store
            .update(
                created.id,
                SavedVisualUpdateRequest {
                    name: Some("Daily Net Revenue".to_string()),
                    description: None,
                    renderer: None,
                    chart_family: None,
                    data_binding: None,
                    visual_spec: None,
                    profile_overlays: None,
                    last_provenance_run_id: Some("prov-123".to_string()),
                },
            )
            .expect("update saved visual");
        assert_eq!(updated.name, "Daily Net Revenue");
        assert_eq!(updated.last_provenance_run_id.as_deref(), Some("prov-123"));

        store.delete(created.id).expect("delete saved visual");
        assert!(matches!(
            store.get(created.id),
            Err(VisualsError::VisualNotFound(_))
        ));
    }
}
