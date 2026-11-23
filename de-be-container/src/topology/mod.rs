//! Semantic & Structural Topology Graph Engine, Dataset/Join Field Anatomy,
//! GraphRAG Layer 2 Overlay Fusion, and Deterministic Mermaid Exporter (Spec 14).

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::analytics::{FieldStatisticalCatalog, LearnedFieldTrait};
use crate::join_probe::JoinProbeResult;
use crate::profiler::{DatasetProfile, FieldProfile};
use crate::provenance::ProvenanceStore;
use crate::visuals::VisualsError;
use crate::visuals::config::VisualsConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogDatasetRecord {
    pub id: String,
    pub canonical_id: String,
    pub source_name: String,
    pub format: String,
    pub grain_description: String,
    pub parquet_path: Option<String>,
    pub profile: DatasetProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JoinHypothesisRecord {
    pub id: String,
    /// "JOINABLE_WITH" | "FORBIDDEN_JOIN" | "DERIVED_FROM"
    pub predicate: String,
    pub epistemic_status: String,
    pub verified_by: Option<String>,
    pub steward_notes: Option<String>,
    pub filter_invariant: Option<String>,
    pub probe: JoinProbeResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinStewardUpdateRequest {
    pub epistemic_status: String,
    #[serde(default)]
    pub verified_by: Option<String>,
    #[serde(default)]
    pub steward_notes: Option<String>,
    #[serde(default)]
    pub filter_invariant: Option<String>,
}

#[derive(Default)]
struct TopologyCatalogInner {
    datasets: BTreeMap<String, CatalogDatasetRecord>,
    joins: BTreeMap<String, JoinHypothesisRecord>,
}

#[derive(Clone, Default)]
pub struct TopologyCatalog {
    inner: Arc<RwLock<TopologyCatalogInner>>,
}

impl TopologyCatalog {
    pub fn upsert_dataset(
        &self,
        source_name: impl Into<String>,
        grain_description: impl Into<String>,
        parquet_path: Option<String>,
        profile: DatasetProfile,
    ) -> Result<CatalogDatasetRecord, VisualsError> {
        let canonical = normalize_id(&profile.dataset_name);
        let rec = CatalogDatasetRecord {
            id: canonical.clone(),
            canonical_id: canonical.clone(),
            source_name: source_name.into(),
            format: "parquet".to_string(),
            grain_description: grain_description.into(),
            parquet_path,
            profile,
        };
        let mut guard = self.inner.write().map_err(|_| VisualsError::LockPoisoned)?;
        guard.datasets.insert(canonical, rec.clone());
        Ok(rec)
    }

    pub fn list_datasets(&self) -> Result<Vec<CatalogDatasetRecord>, VisualsError> {
        let guard = self.inner.read().map_err(|_| VisualsError::LockPoisoned)?;
        Ok(guard.datasets.values().cloned().collect())
    }

    pub fn get_dataset(&self, id_or_name: &str) -> Result<Option<CatalogDatasetRecord>, VisualsError> {
        let guard = self.inner.read().map_err(|_| VisualsError::LockPoisoned)?;
        let key = normalize_id(id_or_name);
        Ok(guard
            .datasets
            .get(id_or_name)
            .or_else(|| guard.datasets.get(&key))
            .cloned())
    }

    pub fn upsert_join_probe(
        &self,
        probe: JoinProbeResult,
        predicate_override: Option<String>,
        steward_notes: Option<String>,
    ) -> Result<JoinHypothesisRecord, VisualsError> {
        let id = format!(
            "join_{}_{}__{}_{}",
            normalize_id(&probe.left_dataset),
            normalize_id(&probe.left_field),
            normalize_id(&probe.right_dataset),
            normalize_id(&probe.right_field)
        );
        let predicate = predicate_override.unwrap_or_else(|| {
            if probe.epistemic_status == "rejected" {
                "FORBIDDEN_JOIN".to_string()
            } else {
                "JOINABLE_WITH".to_string()
            }
        });
        let rec = JoinHypothesisRecord {
            id: id.clone(),
            predicate,
            epistemic_status: probe.epistemic_status.clone(),
            verified_by: None,
            steward_notes,
            filter_invariant: None,
            probe,
        };
        let mut guard = self.inner.write().map_err(|_| VisualsError::LockPoisoned)?;
        guard.joins.insert(id, rec.clone());
        Ok(rec)
    }

    pub fn steward_join(
        &self,
        id: &str,
        req: JoinStewardUpdateRequest,
    ) -> Result<JoinHypothesisRecord, VisualsError> {
        let mut guard = self.inner.write().map_err(|_| VisualsError::LockPoisoned)?;
        let existing = guard
            .joins
            .get_mut(id)
            .ok_or_else(|| VisualsError::TopologyFocusNotFound(id.to_string()))?;
        let status = req.epistemic_status.trim().to_ascii_lowercase();
        existing.epistemic_status = status.clone();
        if status == "rejected" {
            existing.predicate = "FORBIDDEN_JOIN".to_string();
        } else if status == "verified" && existing.predicate == "FORBIDDEN_JOIN" {
            existing.predicate = "JOINABLE_WITH".to_string();
        }
        if let Some(v) = req.verified_by {
            existing.verified_by = Some(v);
        }
        if let Some(n) = req.steward_notes {
            existing.steward_notes = Some(n);
        }
        if let Some(f) = req.filter_invariant {
            existing.filter_invariant = Some(f);
        }
        Ok(existing.clone())
    }

    pub fn list_joins(&self) -> Result<Vec<JoinHypothesisRecord>, VisualsError> {
        let guard = self.inner.read().map_err(|_| VisualsError::LockPoisoned)?;
        Ok(guard.joins.values().cloned().collect())
    }

    pub fn get_join(&self, id: &str) -> Result<Option<JoinHypothesisRecord>, VisualsError> {
        let guard = self.inner.read().map_err(|_| VisualsError::LockPoisoned)?;
        Ok(guard.joins.get(id).cloned())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TopologyGraphQuery {
    #[serde(default)]
    pub lens: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub dataset_ids: Option<Vec<String>>,
    #[serde(default)]
    pub epistemic_status: Option<Vec<String>>,
    #[serde(default)]
    pub focus_node_id: Option<String>,
    #[serde(default)]
    pub hops: Option<u8>,
    #[serde(default)]
    pub include_fields: Option<bool>,
    #[serde(default)]
    pub include_forbidden: Option<bool>,
    #[serde(default)]
    pub include_semantics: Option<bool>,
    #[serde(default)]
    pub provenance_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopologyCommunity {
    pub id: String,
    pub label: String,
    pub member_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopologyNode {
    pub id: String,
    /// "source" | "dataset" | "field" | "business_concept" | "business_metric" | "community_summary"
    pub kind: String,
    pub layer: u8,
    pub parent_id: Option<String>,
    pub label: String,
    pub subtitle: Option<String>,
    pub epistemic_status: String,
    pub confidence: f64,
    pub badges: Vec<String>,
    pub field_count: Option<usize>,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanOutWarning {
    pub multiplier_estimate: f64,
    pub reason: String,
    pub suggested_cte_pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopologyEdge {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub predicate: String,
    pub cardinality: Option<String>,
    pub epistemic_status: String,
    pub confidence: f64,
    pub inclusion_ratio: Option<f64>,
    pub fan_out_warning: Option<FanOutWarning>,
    pub filter_invariant: Option<String>,
    pub steward_notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HighlightedPath {
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub all_rules_human_verified: bool,
    pub fan_out_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticTopologyGraph {
    pub lens: String,
    pub graphrag_overlay_degraded: bool,
    pub truncated: bool,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub applied_filter_hint: Option<String>,
    pub communities: Vec<TopologyCommunity>,
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
    pub highlighted_path: Option<HighlightedPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MiniHistogramBin {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaTreeNode {
    pub name: String,
    pub full_path: String,
    /// "struct" | "list" | "field"
    pub node_kind: String,
    pub arrow_data_type: Option<String>,
    pub is_primary_key_candidate: bool,
    pub epistemic_status: Option<String>,
    pub null_ratio: Option<f64>,
    pub distinct_count: Option<usize>,
    pub top_values: Vec<String>,
    pub mini_histogram_bins: Vec<MiniHistogramBin>,
    pub statistical_profile: Option<LearnedFieldTrait>,
    pub children: Vec<SchemaTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DatasetAnatomyResponse {
    pub dataset_id: String,
    pub canonical_id: String,
    pub format: String,
    pub grain_description: String,
    pub row_count_estimate: usize,
    pub schema_fingerprint_sha256: String,
    pub epistemic_status: String,
    pub schema_tree: Vec<SchemaTreeNode>,
    pub incident_joins: Vec<JoinHypothesisRecord>,
    pub semantic_links: Vec<TopologyNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaCardField {
    pub field_name: String,
    pub arrow_data_type: String,
    pub is_join_key: bool,
    pub is_primary_key_candidate: bool,
    pub null_ratio: f64,
    pub distinct_count: usize,
    pub epistemic_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaCard {
    pub dataset_id: String,
    pub row_count: usize,
    pub join_field: String,
    pub fields: Vec<SchemaCardField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypeCompatibility {
    pub compatible: bool,
    pub left_arrow_type: String,
    pub right_arrow_type: String,
    pub cast_required: Option<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverlapStrip {
    pub jaccard_overlap: f64,
    pub left_inclusion_ratio: f64,
    pub right_inclusion_ratio: f64,
    pub left_distinct_count: usize,
    pub right_distinct_count: usize,
    pub matched_distinct_count: usize,
    pub left_orphan_count: usize,
    pub right_orphan_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanOutDiagnostics {
    pub is_fan_out_risk: bool,
    pub row_multiplier_estimate: f64,
    pub explanation: String,
    pub safe_cte_sql_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JoinAnatomyResponse {
    pub join: JoinHypothesisRecord,
    pub left_dataset: SchemaCard,
    pub right_dataset: SchemaCard,
    pub type_compatibility: TypeCompatibility,
    pub overlap_strip: OverlapStrip,
    pub matched_samples: Vec<String>,
    pub left_orphan_samples: Vec<String>,
    pub right_orphan_samples: Vec<String>,
    pub fan_out_diagnostics: FanOutDiagnostics,
    pub explore_sql_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopologyMermaidResponse {
    pub diagram_kind: String,
    pub mermaid: String,
    pub graph: SemanticTopologyGraph,
}

#[derive(Debug, Deserialize)]
struct GraphragSubgraphNode {
    id: String,
    label: String,
    layer: u8,
    description: String,
    epistemic_status: String,
    confidence: f64,
    verified_by: Option<String>,
    properties: Value,
}

#[derive(Debug, Deserialize)]
struct GraphragSubgraphEdge {
    source_id: String,
    predicate: String,
    target_id: String,
    epistemic_status: String,
    confidence: f64,
    #[serde(default)]
    properties: Value,
}

#[derive(Debug, Deserialize)]
struct GraphragSubgraphCommunity {
    id: String,
    label: String,
    member_node_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GraphragSubgraphResponse {
    #[serde(default)]
    nodes: Vec<GraphragSubgraphNode>,
    #[serde(default)]
    edges: Vec<GraphragSubgraphEdge>,
    #[serde(default)]
    communities: Vec<GraphragSubgraphCommunity>,
}

fn normalize_id(raw: &str) -> String {
    let mut slug = String::with_capacity(raw.len());
    let mut prev_us = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_us = false;
        } else if !prev_us && !slug.is_empty() {
            slug.push('_');
            prev_us = true;
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    slug
}

fn decode_chunked_body(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut idx = 0_usize;
    while idx < raw.len() {
        let line_end = raw[idx..].windows(2).position(|w| w == b"\r\n")?;
        let size_str = std::str::from_utf8(&raw[idx..idx + line_end]).ok()?.trim();
        let hex_part = size_str.split(';').next().unwrap_or(size_str).trim();
        let chunk_len = usize::from_str_radix(hex_part, 16).ok()?;
        idx += line_end + 2;
        if chunk_len == 0 {
            break;
        }
        if idx + chunk_len > raw.len() {
            return None;
        }
        out.extend_from_slice(&raw[idx..idx + chunk_len]);
        idx += chunk_len;
        if idx + 2 <= raw.len() && &raw[idx..idx + 2] == b"\r\n" {
            idx += 2;
        }
    }
    Some(out)
}

async fn fetch_graphrag_subgraph(
    base_url: &str,
    dataset_ids: &[String],
    hops: u8,
    include_forbidden: bool,
) -> Result<GraphragSubgraphResponse, VisualsError> {
    let stripped = base_url
        .trim()
        .strip_prefix("http://")
        .ok_or_else(|| {
            VisualsError::GraphragOverlay(
                "only http:// GraphRAG URLs are supported by internal overlay client".to_string(),
            )
        })?;
    let (host_port, base_path) = match stripped.find('/') {
        Some(pos) => (&stripped[..pos], stripped[pos..].trim_end_matches('/')),
        None => (stripped, ""),
    };
    let addr = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{}:80", host_port)
    };

    let mut qs = format!("hops={}&include_forbidden={}", hops, include_forbidden);
    if !dataset_ids.is_empty() {
        qs.push_str("&dataset_ids=");
        qs.push_str(&dataset_ids.join(","));
    }
    let path_and_query = format!("{}/ontology/subgraph?{}", base_path, qs);

    let fut = async {
        let mut stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| VisualsError::GraphragOverlay(format!("connect failed: {}", e)))?;
        let req = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            path_and_query, host_port
        );
        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| VisualsError::GraphragOverlay(format!("write failed: {}", e)))?;
        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|e| VisualsError::GraphragOverlay(format!("read failed: {}", e)))?;

        let header_end = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| VisualsError::GraphragOverlay("malformed HTTP response".to_string()))?;
        let header_str = std::str::from_utf8(&buf[..header_end])
            .map_err(|e| VisualsError::GraphragOverlay(format!("invalid utf8 headers: {}", e)))?;
        let status_line = header_str.lines().next().unwrap_or("");
        if !status_line.contains(" 200 ") {
            return Err(VisualsError::GraphragOverlay(format!(
                "non-200 status from Graphrag: {}",
                status_line
            )));
        }
        let is_chunked = header_str
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked");
        let body_slice = &buf[header_end + 4..];
        let body_bytes = if is_chunked {
            decode_chunked_body(body_slice).ok_or_else(|| {
                VisualsError::GraphragOverlay("failed to decode chunked HTTP body".to_string())
            })?
        } else {
            body_slice.to_vec()
        };
        serde_json::from_slice::<GraphragSubgraphResponse>(&body_bytes)
            .map_err(|e| VisualsError::GraphragOverlay(format!("invalid Graphrag JSON: {}", e)))
    };

    tokio::time::timeout(Duration::from_secs(2), fut)
        .await
        .map_err(|_| {
            VisualsError::GraphragOverlay("Graphrag subgraph request timed out".to_string())
        })?
}

fn epistemic_priority(status: &str) -> u8 {
    match status {
        "verified" => 0,
        "drifted" => 1,
        "empirically_tested" => 2,
        "pending_review" => 3,
        "hypothesized" => 4,
        "discovered" => 5,
        "rejected" => 6,
        _ => 7,
    }
}

/// Builds a fused, bounded multi-layer `SemanticTopologyGraph` combining `data-engine`'s catalog,
/// statistical field traits/confounders, provenance lineage, and `Graphrag`'s Layer 2 ontology.
pub async fn build_topology_graph(
    catalog: &TopologyCatalog,
    field_catalog: &FieldStatisticalCatalog,
    provenance_store: &ProvenanceStore,
    config: &VisualsConfig,
    query: &TopologyGraphQuery,
) -> Result<SemanticTopologyGraph, VisualsError> {
    let lens = query
        .lens
        .clone()
        .unwrap_or_else(|| "semantic_3layer".to_string());
    let hops = query.hops.unwrap_or(config.default_ego_hops);
    let include_forbidden = query.include_forbidden.unwrap_or(true);
    let include_semantics = query.include_semantics.unwrap_or(true);

    let all_datasets = catalog.list_datasets()?;
    let all_joins = catalog.list_joins()?;
    let all_traits = field_catalog.list()?;
    let trait_map: HashMap<String, LearnedFieldTrait> = all_traits
        .into_iter()
        .map(|t| (t.field_canonical_id.clone(), t))
        .collect();

    let dataset_filter: Option<HashSet<String>> = query
        .dataset_ids
        .as_ref()
        .map(|v| v.iter().map(|s| normalize_id(s)).collect());

    let scoped_datasets: Vec<CatalogDatasetRecord> = all_datasets
        .into_iter()
        .filter(|ds| {
            if let Some(ref src) = query.source_id
                && ds.source_name != *src
            {
                return false;
            }
            if let Some(ref set) = dataset_filter
                && !set.contains(&ds.canonical_id)
            {
                return false;
            }
            true
        })
        .collect();

    let include_fields = query
        .include_fields
        .unwrap_or(scoped_datasets.len() <= 15);

    let mut nodes_map: BTreeMap<String, TopologyNode> = BTreeMap::new();
    let mut edges_vec: Vec<TopologyEdge> = Vec::new();
    let mut communities: Vec<TopologyCommunity> = Vec::new();

    // 1. Add Layer 1 DataSource & Dataset nodes + Layer 0 DatasetField nodes
    let mut source_ids_seen: HashSet<String> = HashSet::new();
    for ds in &scoped_datasets {
        let src_node_id = format!("src_{}", normalize_id(&ds.source_name));
        if source_ids_seen.insert(src_node_id.clone()) {
            nodes_map.insert(
                src_node_id.clone(),
                TopologyNode {
                    id: src_node_id.clone(),
                    kind: "source".to_string(),
                    layer: 1,
                    parent_id: None,
                    label: ds.source_name.clone(),
                    subtitle: Some("Registered Data Source".to_string()),
                    epistemic_status: "verified".to_string(),
                    confidence: 1.0,
                    badges: vec!["source".to_string()],
                    field_count: None,
                    metadata: json!({ "source_name": ds.source_name }),
                },
            );
        }

        let mut ds_badges = vec![ds.format.clone()];
        if ds.profile.epistemic_status == "drifted" || !ds.profile.drift_findings.is_empty() {
            ds_badges.push("drifted".to_string());
        }
        let ds_status = if !ds.profile.drift_findings.is_empty() {
            "drifted".to_string()
        } else {
            ds.profile.epistemic_status.clone()
        };

        nodes_map.insert(
            ds.canonical_id.clone(),
            TopologyNode {
                id: ds.canonical_id.clone(),
                kind: "dataset".to_string(),
                layer: 1,
                parent_id: Some(src_node_id.clone()),
                label: ds.profile.dataset_name.clone(),
                subtitle: Some(format!(
                    "{} • {} rows",
                    ds.grain_description, ds.profile.row_count
                )),
                epistemic_status: ds_status,
                confidence: 1.0,
                badges: ds_badges,
                field_count: Some(ds.profile.fields.len()),
                metadata: json!({
                    "format": ds.format,
                    "row_count_estimate": ds.profile.row_count,
                    "grain_description": ds.grain_description,
                    "schema_fingerprint_sha256": ds.profile.schema_fingerprint_sha256,
                    "drift_findings": ds.profile.drift_findings,
                }),
            },
        );
        edges_vec.push(TopologyEdge {
            id: format!("provides_{}_{}", src_node_id, ds.canonical_id),
            source_id: src_node_id,
            target_id: ds.canonical_id.clone(),
            predicate: "PROVIDES".to_string(),
            cardinality: None,
            epistemic_status: "verified".to_string(),
            confidence: 1.0,
            inclusion_ratio: None,
            fan_out_warning: None,
            filter_invariant: None,
            steward_notes: None,
        });

        let drifted_fields: HashSet<&str> = ds
            .profile
            .drift_findings
            .iter()
            .map(|d| d.field_name.as_str())
            .collect();

        if include_fields {
            for field in &ds.profile.fields {
                let canonical_field = format!("{}.{}", ds.canonical_id, field.field_name);
                let stat_trait = trait_map.get(&canonical_field);
                let mut f_badges = vec![field.arrow_data_type.clone()];
                if ds
                    .profile
                    .candidate_primary_keys
                    .contains(&field.field_name)
                {
                    f_badges.push("pk".to_string());
                }
                let f_status = if drifted_fields.contains(field.field_name.as_str()) {
                    f_badges.push("drifted".to_string());
                    "drifted".to_string()
                } else {
                    field.epistemic_status.clone()
                };
                if let Some(tr) = stat_trait {
                    if tr.is_heavy_tailed {
                        f_badges.push("heavy_tail".to_string());
                    }
                    if tr.seasonality_period.is_some() {
                        f_badges.push("seasonal".to_string());
                    }
                }
                nodes_map.insert(
                    canonical_field.clone(),
                    TopologyNode {
                        id: canonical_field.clone(),
                        kind: "field".to_string(),
                        layer: 0,
                        parent_id: Some(ds.canonical_id.clone()),
                        label: field.field_name.clone(),
                        subtitle: Some(format!(
                            "{} • {:.0}% null",
                            field.arrow_data_type,
                            field.null_ratio * 100.0
                        )),
                        epistemic_status: f_status,
                        confidence: stat_trait.map(|t| t.confidence).unwrap_or(1.0),
                        badges: f_badges,
                        field_count: None,
                        metadata: json!({
                            "arrow_data_type": field.arrow_data_type,
                            "null_ratio": field.null_ratio,
                            "distinct_count": field.distinct_count,
                            "inferred_semantic_domain": field.inferred_semantic_domain,
                            "statistical_profile": stat_trait,
                        }),
                    },
                );
                edges_vec.push(TopologyEdge {
                    id: format!("has_field_{}", canonical_field),
                    source_id: ds.canonical_id.clone(),
                    target_id: canonical_field.clone(),
                    predicate: "HAS_FIELD".to_string(),
                    cardinality: None,
                    epistemic_status: "verified".to_string(),
                    confidence: 1.0,
                    inclusion_ratio: None,
                    fan_out_warning: None,
                    filter_invariant: None,
                    steward_notes: None,
                });

                // Emit local CONFOUNDED_BY edges from FieldStatisticalCatalog
                if let Some(tr) = stat_trait {
                    for conf in &tr.known_confounders {
                        edges_vec.push(TopologyEdge {
                            id: format!("confounded_{}_{}", canonical_field, conf),
                            source_id: canonical_field.clone(),
                            target_id: conf.clone(),
                            predicate: "CONFOUNDED_BY".to_string(),
                            cardinality: None,
                            epistemic_status: tr.epistemic_status.clone(),
                            confidence: tr.confidence,
                            inclusion_ratio: None,
                            fan_out_warning: None,
                            filter_invariant: None,
                            steward_notes: Some("Statistical confounder (Simpson's paradox guard)".to_string()),
                        });
                    }
                }
            }
        }
    }

    // 2. Count outgoing 1:N / N:M joins per dataset to detect TRAP-03 multi-child chasm fan-outs
    let mut multi_cardinality_counts: HashMap<String, usize> = HashMap::new();
    for j in &all_joins {
        if j.predicate == "JOINABLE_WITH"
            && (j.probe.observed_cardinality == "1:N"
                || j.probe.observed_cardinality == "N:M"
                || j.probe.fan_out_risk)
        {
            *multi_cardinality_counts
                .entry(normalize_id(&j.probe.left_dataset))
                .or_insert(0) += 1;
        }
    }

    for j in &all_joins {
        if !include_forbidden && j.predicate == "FORBIDDEN_JOIN" {
            continue;
        }
        let left_ds = normalize_id(&j.probe.left_dataset);
        let right_ds = normalize_id(&j.probe.right_dataset);
        let left_endpoint = if include_fields {
            format!("{}.{}", left_ds, j.probe.left_field)
        } else {
            left_ds.clone()
        };
        let right_endpoint = if include_fields {
            format!("{}.{}", right_ds, j.probe.right_field)
        } else {
            right_ds.clone()
        };
        if !nodes_map.contains_key(&left_endpoint) || !nodes_map.contains_key(&right_endpoint) {
            continue;
        }

        let is_multi_chasm =
            multi_cardinality_counts.get(&left_ds).copied().unwrap_or(0) >= 2;
        let fan_out_warning = if j.predicate == "JOINABLE_WITH"
            && (j.probe.fan_out_risk
                || j.probe.observed_cardinality == "N:M"
                || is_multi_chasm)
        {
            Some(FanOutWarning {
                multiplier_estimate: j.probe.row_multiplication_factor.max(1.5),
                reason: if is_multi_chasm {
                    format!(
                        "Chasm fan-out risk: '{}' joins to multiple 1:N child tables; pre-aggregate before joining.",
                        left_ds
                    )
                } else {
                    format!(
                        "Fan-out risk ({} cardinality, {:.2}x row multiplication).",
                        j.probe.observed_cardinality, j.probe.row_multiplication_factor
                    )
                },
                suggested_cte_pattern: format!(
                    "WITH agg_{r} AS (SELECT {rf}, SUM(metric) AS total FROM {r} GROUP BY {rf}) SELECT ... FROM {l} JOIN agg_{r} ON {l}.{lf} = agg_{r}.{rf}",
                    l = left_ds,
                    lf = j.probe.left_field,
                    r = right_ds,
                    rf = j.probe.right_field
                ),
            })
        } else {
            None
        };

        edges_vec.push(TopologyEdge {
            id: j.id.clone(),
            source_id: left_endpoint,
            target_id: right_endpoint,
            predicate: j.predicate.clone(),
            cardinality: Some(j.probe.observed_cardinality.clone()),
            epistemic_status: j.epistemic_status.clone(),
            confidence: j.probe.confidence,
            inclusion_ratio: Some(j.probe.inclusion_ratio_left_in_right),
            fan_out_warning,
            filter_invariant: j.filter_invariant.clone(),
            steward_notes: j.steward_notes.clone(),
        });
    }

    // 3. Fuse GraphRAG Layer 2 semantic ontology overlay (with graceful degradation)
    let mut graphrag_overlay_degraded = false;
    if include_semantics {
        if let Some(ref base_url) = config.graphrag_base_url {
            let ds_names: Vec<String> = scoped_datasets
                .iter()
                .map(|d| d.canonical_id.clone())
                .collect();
            match fetch_graphrag_subgraph(base_url.as_str(), &ds_names, hops, include_forbidden).await {
                Ok(gr) => {
                    // Build lookup from GraphRAG field node id -> canonical "ds.field" id
                    let mut gr_id_remap: HashMap<String, String> = HashMap::new();
                    for gn in &gr.nodes {
                        if let Some(canon) = gn
                            .properties
                            .get("canonical_id")
                            .and_then(|v| v.as_str())
                            && nodes_map.contains_key(canon)
                        {
                            gr_id_remap.insert(gn.id.clone(), canon.to_string());
                        }
                    }

                    for gn in gr.nodes {
                        let mapped_id = gr_id_remap
                            .get(&gn.id)
                            .cloned()
                            .unwrap_or_else(|| gn.id.clone());
                        if nodes_map.contains_key(&mapped_id) {
                            continue;
                        }
                        let kind = match gn.label.as_str() {
                            "BusinessConcept" => "business_concept",
                            "BusinessMetric" => "business_metric",
                            "CommunitySummary" => "community_summary",
                            "Dataset" => "dataset",
                            "Field" => "field",
                            _ => "business_concept",
                        };
                        nodes_map.insert(
                            mapped_id.clone(),
                            TopologyNode {
                                id: mapped_id,
                                kind: kind.to_string(),
                                layer: gn.layer,
                                parent_id: None,
                                label: gn.id,
                                subtitle: Some(gn.description),
                                epistemic_status: gn.epistemic_status,
                                confidence: gn.confidence,
                                badges: vec![gn.label],
                                field_count: None,
                                metadata: json!({
                                    "verified_by": gn.verified_by,
                                    "properties": gn.properties,
                                }),
                            },
                        );
                    }

                    for ge in gr.edges {
                        let src = gr_id_remap
                            .get(&ge.source_id)
                            .cloned()
                            .unwrap_or(ge.source_id);
                        let tgt = gr_id_remap
                            .get(&ge.target_id)
                            .cloned()
                            .unwrap_or(ge.target_id);
                        if !nodes_map.contains_key(&src) || !nodes_map.contains_key(&tgt) {
                            continue;
                        }
                        let edge_id = format!("gr_{}_{}_{}", src, ge.predicate, tgt);
                        if edges_vec
                            .iter()
                            .any(|e| e.source_id == src && e.predicate == ge.predicate && e.target_id == tgt)
                        {
                            continue;
                        }
                        edges_vec.push(TopologyEdge {
                            id: edge_id,
                            source_id: src,
                            target_id: tgt,
                            predicate: ge.predicate,
                            cardinality: ge
                                .properties
                                .get("cardinality")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                            epistemic_status: ge.epistemic_status,
                            confidence: ge.confidence,
                            inclusion_ratio: ge
                                .properties
                                .get("inclusion_ratio")
                                .and_then(|v| v.as_f64()),
                            fan_out_warning: None,
                            filter_invariant: None,
                            steward_notes: ge
                                .properties
                                .get("rejection_reason")
                                .and_then(|v| v.as_str())
                                .map(str::to_string),
                        });
                    }

                    for gc in gr.communities {
                        communities.push(TopologyCommunity {
                            id: gc.id,
                            label: gc.label,
                            member_node_ids: gc
                                .member_node_ids
                                .into_iter()
                                .map(|m| gr_id_remap.get(&m).cloned().unwrap_or(m))
                                .collect(),
                        });
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "GraphRAG overlay fetch failed; returning topology in degraded mode"
                    );
                    graphrag_overlay_degraded = true;
                }
            }
        }
    }

    // 4. Optional focus ego-network BFS
    if let Some(ref focus) = query.focus_node_id {
        let focus_key = if nodes_map.contains_key(focus) {
            focus.clone()
        } else {
            let norm = normalize_id(focus);
            if nodes_map.contains_key(&norm) {
                norm
            } else {
                return Err(VisualsError::TopologyFocusNotFound(focus.clone()));
            }
        };
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &edges_vec {
            adj.entry(e.source_id.as_str()).or_default().push(e.target_id.as_str());
            adj.entry(e.target_id.as_str()).or_default().push(e.source_id.as_str());
        }
        let mut visited: HashSet<String> = HashSet::new();
        let mut q: VecDeque<(String, u8)> = VecDeque::new();
        visited.insert(focus_key.clone());
        q.push_back((focus_key, 0));
        while let Some((curr, dist)) = q.pop_front() {
            if dist >= hops {
                continue;
            }
            if let Some(nbrs) = adj.get(curr.as_str()) {
                for &n in nbrs {
                    if visited.insert(n.to_string()) {
                        q.push_back((n.to_string(), dist + 1));
                    }
                }
            }
        }
        nodes_map.retain(|k, _| visited.contains(k));
    }

    // 5. Optional epistemic_status filter
    if let Some(ref statuses) = query.epistemic_status
        && !statuses.is_empty()
    {
        let allowed: HashSet<String> = statuses
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .collect();
        nodes_map.retain(|_, n| {
            n.kind == "source" || allowed.contains(&n.epistemic_status.to_ascii_lowercase())
        });
    }

    // 6. Deterministic sort + bounds enforcement (zero dangling edges)
    let mut nodes: Vec<TopologyNode> = nodes_map.into_values().collect();
    nodes.sort_by(|a, b| {
        (Reverse(a.layer), epistemic_priority(&a.epistemic_status), &a.id).cmp(&(
            Reverse(b.layer),
            epistemic_priority(&b.epistemic_status),
            &b.id,
        ))
    });
    let total_nodes = nodes.len();
    let nodes_truncated = total_nodes > config.max_topology_nodes;
    nodes.truncate(config.max_topology_nodes);

    let kept_ids: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let mut filtered_edges: Vec<TopologyEdge> = edges_vec
        .into_iter()
        .filter(|e| {
            kept_ids.contains(e.source_id.as_str()) && kept_ids.contains(e.target_id.as_str())
        })
        .collect();
    filtered_edges.sort_by(|a, b| {
        (&a.source_id, &a.predicate, &a.target_id).cmp(&(&b.source_id, &b.predicate, &b.target_id))
    });
    let total_edges = filtered_edges.len();
    let edges_truncated = total_edges > config.max_topology_edges;
    filtered_edges.truncate(config.max_topology_edges);

    let truncated = nodes_truncated || edges_truncated;
    let applied_filter_hint = if truncated {
        Some("Graph exceeded configured bounds; pass focus_node_id, dataset_ids, or include_fields=false to narrow scope.".to_string())
    } else {
        None
    };

    // 7. Optional ProvenanceRun lineage overlay highlight
    let highlighted_path = if let Some(ref run_id) = query.provenance_run_id {
        if let Ok(run) = provenance_store.get_run(run_id) {
            let mut h_nodes: Vec<String> = Vec::new();
            for ds in &run.datasets_consulted {
                let norm = normalize_id(ds);
                if kept_ids.contains(norm.as_str()) {
                    h_nodes.push(norm);
                }
            }
            let h_set: HashSet<&str> = h_nodes.iter().map(String::as_str).collect();
            let h_edges: Vec<String> = filtered_edges
                .iter()
                .filter(|e| h_set.contains(e.source_id.as_str()) || h_set.contains(e.target_id.as_str()))
                .filter(|e| e.predicate == "JOINABLE_WITH" || e.predicate == "COMPUTED_FROM")
                .map(|e| e.id.clone())
                .collect();
            let warnings: Vec<String> = filtered_edges
                .iter()
                .filter(|e| h_edges.contains(&e.id))
                .filter_map(|e| e.fan_out_warning.as_ref().map(|w| w.reason.clone()))
                .collect();
            Some(HighlightedPath {
                node_ids: h_nodes,
                edge_ids: h_edges,
                all_rules_human_verified: run.trust_summary.all_rules_human_verified,
                fan_out_warnings: warnings,
            })
        } else {
            None
        }
    } else {
        None
    };

    Ok(SemanticTopologyGraph {
        lens,
        graphrag_overlay_degraded,
        truncated,
        total_nodes,
        total_edges,
        applied_filter_hint,
        communities,
        nodes,
        edges: filtered_edges,
        highlighted_path,
    })
}

fn insert_schema_path(
    roots: &mut Vec<SchemaTreeNode>,
    segments: &[&str],
    prefix: &str,
    field: &FieldProfile,
    is_pk: bool,
    stat_trait: Option<LearnedFieldTrait>,
) {
    if segments.is_empty() {
        return;
    }
    let seg = segments[0];
    let is_list = seg.ends_with("[]");
    let full_path = if prefix.is_empty() {
        seg.to_string()
    } else {
        format!("{}.{}", prefix, seg)
    };

    if segments.len() == 1 {
        let bins: Vec<MiniHistogramBin> = field
            .top_k_values
            .iter()
            .take(5)
            .enumerate()
            .map(|(i, v)| MiniHistogramBin {
                label: v.clone(),
                count: (5 - i) * 10,
            })
            .collect();
        roots.push(SchemaTreeNode {
            name: seg.to_string(),
            full_path,
            node_kind: "field".to_string(),
            arrow_data_type: Some(field.arrow_data_type.clone()),
            is_primary_key_candidate: is_pk,
            epistemic_status: Some(field.epistemic_status.clone()),
            null_ratio: Some(field.null_ratio),
            distinct_count: Some(field.distinct_count),
            top_values: field.top_k_values.clone(),
            mini_histogram_bins: bins,
            statistical_profile: stat_trait,
            children: vec![],
        });
        return;
    }

    let existing_pos = roots.iter().position(|n| n.name == seg && n.node_kind != "field");
    let container_idx = if let Some(pos) = existing_pos {
        pos
    } else {
        roots.push(SchemaTreeNode {
            name: seg.to_string(),
            full_path: full_path.clone(),
            node_kind: if is_list { "list" } else { "struct" }.to_string(),
            arrow_data_type: Some(if is_list { "List<Struct>" } else { "Struct" }.to_string()),
            is_primary_key_candidate: false,
            epistemic_status: None,
            null_ratio: None,
            distinct_count: None,
            top_values: vec![],
            mini_histogram_bins: vec![],
            statistical_profile: None,
            children: vec![],
        });
        roots.len() - 1
    };

    insert_schema_path(
        &mut roots[container_idx].children,
        &segments[1..],
        &full_path,
        field,
        is_pk,
        stat_trait,
    );
}

/// Builds zoomed-in structural anatomy for one dataset (nested Arrow schema tree + profile bins + incident joins).
pub fn build_dataset_anatomy(
    dataset_id_or_name: &str,
    catalog: &TopologyCatalog,
    field_catalog: &FieldStatisticalCatalog,
) -> Result<DatasetAnatomyResponse, VisualsError> {
    let ds = catalog
        .get_dataset(dataset_id_or_name)?
        .ok_or_else(|| VisualsError::TopologyFocusNotFound(dataset_id_or_name.to_string()))?;

    let mut schema_tree: Vec<SchemaTreeNode> = Vec::new();
    for field in &ds.profile.fields {
        let canonical = format!("{}.{}", ds.canonical_id, field.field_name);
        let stat_trait = field_catalog.get(&canonical)?;
        let is_pk = ds.profile.candidate_primary_keys.contains(&field.field_name);
        let segments: Vec<&str> = field.field_name.split('.').collect();
        insert_schema_path(&mut schema_tree, &segments, "", field, is_pk, stat_trait);
    }

    let incident_joins: Vec<JoinHypothesisRecord> = catalog
        .list_joins()?
        .into_iter()
        .filter(|j| {
            normalize_id(&j.probe.left_dataset) == ds.canonical_id
                || normalize_id(&j.probe.right_dataset) == ds.canonical_id
        })
        .collect();

    Ok(DatasetAnatomyResponse {
        dataset_id: ds.id,
        canonical_id: ds.canonical_id,
        format: ds.format,
        grain_description: ds.grain_description,
        row_count_estimate: ds.profile.row_count,
        schema_fingerprint_sha256: ds.profile.schema_fingerprint_sha256,
        epistemic_status: ds.profile.epistemic_status,
        schema_tree,
        incident_joins,
        semantic_links: vec![],
    })
}

fn build_schema_card(
    dataset_name: &str,
    join_field: &str,
    fallback_row_count: usize,
    catalog: &TopologyCatalog,
) -> (SchemaCard, String) {
    if let Ok(Some(ds)) = catalog.get_dataset(dataset_name) {
        let mut join_type = "Unknown".to_string();
        let fields = ds
            .profile
            .fields
            .iter()
            .map(|f| {
                let is_key = f.field_name == join_field;
                if is_key {
                    join_type = f.arrow_data_type.clone();
                }
                SchemaCardField {
                    field_name: f.field_name.clone(),
                    arrow_data_type: f.arrow_data_type.clone(),
                    is_join_key: is_key,
                    is_primary_key_candidate: ds
                        .profile
                        .candidate_primary_keys
                        .contains(&f.field_name),
                    null_ratio: f.null_ratio,
                    distinct_count: f.distinct_count,
                    epistemic_status: f.epistemic_status.clone(),
                }
            })
            .collect();
        (
            SchemaCard {
                dataset_id: ds.canonical_id,
                row_count: ds.profile.row_count,
                join_field: join_field.to_string(),
                fields,
            },
            join_type,
        )
    } else {
        (
            SchemaCard {
                dataset_id: normalize_id(dataset_name),
                row_count: fallback_row_count,
                join_field: join_field.to_string(),
                fields: vec![SchemaCardField {
                    field_name: join_field.to_string(),
                    arrow_data_type: "Unknown".to_string(),
                    is_join_key: true,
                    is_primary_key_candidate: false,
                    null_ratio: 0.0,
                    distinct_count: 0,
                    epistemic_status: "discovered".to_string(),
                }],
            },
            "Unknown".to_string(),
        )
    }
}

/// Builds zoomed-in Field-to-Field Join Anatomy with schema cards, Arrow type compatibility,
/// Venn/Sankey overlap strip, fan-out diagnostics, and ready-to-run `explore_sql_template`.
pub fn build_join_anatomy(
    join_id: &str,
    catalog: &TopologyCatalog,
) -> Result<JoinAnatomyResponse, VisualsError> {
    let join = catalog
        .get_join(join_id)?
        .ok_or_else(|| VisualsError::TopologyFocusNotFound(join_id.to_string()))?;

    let (left_card, left_type) = build_schema_card(
        &join.probe.left_dataset,
        &join.probe.left_field,
        join.probe.left_row_count,
        catalog,
    );
    let (right_card, right_type) = build_schema_card(
        &join.probe.right_dataset,
        &join.probe.right_field,
        join.probe.right_row_count,
        catalog,
    );

    let compatible = left_type == right_type || left_type == "Unknown" || right_type == "Unknown";
    let (cast_required, type_warning) = if compatible {
        (None, None)
    } else {
        (
            Some(format!("CAST(l.{} AS {})", join.probe.left_field, right_type)),
            Some(format!(
                "Arrow type mismatch: {}.{} is '{}' while {}.{} is '{}'; explicit cast required.",
                join.probe.left_dataset,
                join.probe.left_field,
                left_type,
                join.probe.right_dataset,
                join.probe.right_field,
                right_type
            )),
        )
    };

    let matched_distinct = ((join.probe.left_distinct_count as f64)
        * join.probe.inclusion_ratio_left_in_right)
        .round() as usize;
    let matched_distinct = matched_distinct
        .min(join.probe.left_distinct_count)
        .min(join.probe.right_distinct_count);
    let left_orphans = join
        .probe
        .left_distinct_count
        .saturating_sub(matched_distinct);
    let right_orphans = join
        .probe
        .right_distinct_count
        .saturating_sub(matched_distinct);

    let is_fan_out = join.probe.fan_out_risk || join.probe.observed_cardinality == "N:M";
    let safe_cte_sql_hint = if is_fan_out {
        Some(format!(
            "WITH right_agg AS (SELECT {rf}, COUNT(*) AS right_rows FROM {r} GROUP BY {rf}) SELECT l.*, right_agg.right_rows FROM {l} l LEFT JOIN right_agg ON l.{lf} = right_agg.{rf}",
            l = left_card.dataset_id,
            lf = join.probe.left_field,
            r = right_card.dataset_id,
            rf = join.probe.right_field
        ))
    } else {
        None
    };

    let explore_sql_template = format!(
        "SELECT l.*, r.* FROM {} l JOIN {} r ON l.{} = r.{} LIMIT 1000",
        left_card.dataset_id,
        right_card.dataset_id,
        join.probe.left_field,
        join.probe.right_field
    );

    Ok(JoinAnatomyResponse {
        left_dataset: left_card,
        right_dataset: right_card,
        type_compatibility: TypeCompatibility {
            compatible,
            left_arrow_type: left_type,
            right_arrow_type: right_type,
            cast_required,
            warning: type_warning,
        },
        overlap_strip: OverlapStrip {
            jaccard_overlap: join.probe.jaccard_similarity,
            left_inclusion_ratio: join.probe.inclusion_ratio_left_in_right,
            right_inclusion_ratio: join.probe.inclusion_ratio_right_in_left,
            left_distinct_count: join.probe.left_distinct_count,
            right_distinct_count: join.probe.right_distinct_count,
            matched_distinct_count: matched_distinct,
            left_orphan_count: left_orphans,
            right_orphan_count: right_orphans,
        },
        matched_samples: vec![],
        left_orphan_samples: vec![],
        right_orphan_samples: vec![],
        fan_out_diagnostics: FanOutDiagnostics {
            is_fan_out_risk: is_fan_out,
            row_multiplier_estimate: join.probe.row_multiplication_factor,
            explanation: join.probe.explanation.clone(),
            safe_cte_sql_hint,
        },
        explore_sql_template,
        join,
    })
}

/// Deterministically renders a scoped `SemanticTopologyGraph` into Mermaid `erDiagram` or `flowchart TB`.
pub fn export_topology_mermaid(graph: SemanticTopologyGraph) -> TopologyMermaidResponse {
    if graph.lens == "er_join" {
        let mut lines = vec!["erDiagram".to_string()];
        let datasets: Vec<&TopologyNode> = graph
            .nodes
            .iter()
            .filter(|n| n.kind == "dataset")
            .collect();
        for ds in &datasets {
            let entity_id = normalize_id(&ds.id).to_ascii_uppercase();
            lines.push(format!("    {} {{", entity_id));
            for field in graph
                .nodes
                .iter()
                .filter(|f| f.kind == "field" && f.parent_id.as_deref() == Some(ds.id.as_str()))
            {
                let dt = field
                    .metadata
                    .get("arrow_data_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string");
                let pk = if field.badges.iter().any(|b| b == "pk") {
                    " PK"
                } else {
                    ""
                };
                lines.push(format!("        {} {}{}", dt, field.label, pk));
            }
            lines.push("    }".to_string());
        }
        for e in &graph.edges {
            if e.predicate != "JOINABLE_WITH" && e.predicate != "FORBIDDEN_JOIN" {
                continue;
            }
            let left_ds = e
                .source_id
                .split('.')
                .next()
                .unwrap_or(&e.source_id)
                .to_ascii_uppercase();
            let right_ds = e
                .target_id
                .split('.')
                .next()
                .unwrap_or(&e.target_id)
                .to_ascii_uppercase();
            if e.predicate == "FORBIDDEN_JOIN" {
                lines.push(format!(
                    "    %% FORBIDDEN_JOIN: {} x {} ({})",
                    e.source_id,
                    e.target_id,
                    e.steward_notes.as_deref().unwrap_or("rejected")
                ));
            } else {
                let rel = match e.cardinality.as_deref() {
                    Some("1:1") => "||--||",
                    Some("1:N") => "||--o{",
                    Some("N:1") => "}o--||",
                    _ => "}o--o{",
                };
                lines.push(format!(
                    "    {} {} {} : \"{} ({})\"",
                    left_ds,
                    rel,
                    right_ds,
                    e.cardinality.as_deref().unwrap_or("join"),
                    e.epistemic_status
                ));
            }
        }
        TopologyMermaidResponse {
            diagram_kind: "erDiagram".to_string(),
            mermaid: lines.join("\n"),
            graph,
        }
    } else {
        let mut lines = vec!["flowchart TB".to_string()];
        let mut l2 = Vec::new();
        let mut l1 = Vec::new();
        let mut l0 = Vec::new();
        for n in &graph.nodes {
            let safe_id = format!("n_{}", normalize_id(&n.id));
            let label = format!("{} [{}]", n.label, n.epistemic_status);
            let line = format!("        {}[\"{}\"]", safe_id, label);
            match n.layer {
                2 => l2.push(line),
                1 => l1.push(line),
                _ => l0.push(line),
            }
        }
        if !l2.is_empty() {
            lines.push("    subgraph Layer2[\"Layer 2: Semantic Ontology\"]".to_string());
            lines.extend(l2);
            lines.push("    end".to_string());
        }
        if !l1.is_empty() {
            lines.push("    subgraph Layer1[\"Layer 1: Datasets\"]".to_string());
            lines.extend(l1);
            lines.push("    end".to_string());
        }
        if !l0.is_empty() {
            lines.push("    subgraph Layer0[\"Layer 0: Physical Fields\"]".to_string());
            lines.extend(l0);
            lines.push("    end".to_string());
        }
        for e in &graph.edges {
            let s = format!("n_{}", normalize_id(&e.source_id));
            let t = format!("n_{}", normalize_id(&e.target_id));
            let arrow = if e.predicate == "FORBIDDEN_JOIN" || e.predicate == "CONFOUNDED_BY" {
                format!("-. \"{} [{}]\" .->", e.predicate, e.epistemic_status)
            } else {
                format!("-->|\"{} [{}]\"|", e.predicate, e.epistemic_status)
            };
            lines.push(format!("    {} {} {}", s, arrow, t));
        }
        TopologyMermaidResponse {
            diagram_kind: "flowchart".to_string(),
            mermaid: lines.join("\n"),
            graph,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::{Json, Router};
    use tokio::net::TcpListener;

    use crate::config::{ComputeConfig, ProfilerConfig};
    use crate::join_probe::probe_join;
    use crate::profiler::profile_parquet_dataset;
    use crate::seeder::seed_validation_suite_with_options;
    use tempfile::tempdir;

    fn sample_profiler_cfg() -> ProfilerConfig {
        ProfilerConfig {
            max_sample_rows: 50_000,
            top_k_values: 5,
            minhash_num_hashes: 32,
            drift_relative_mean_threshold: 5.0,
        }
    }

    fn sample_compute_cfg() -> ComputeConfig {
        ComputeConfig {
            query_timeout_secs: 30,
            max_preview_rows: 100,
            join_inclusion_threshold: 0.80,
        }
    }

    #[tokio::test]
    async fn test_topology_graph_fuses_catalog_and_mock_graphrag_layers() {
        // Start local mock GraphRAG HTTP server returning Layer 2 BusinessMetric & Community
        let mock_app = Router::new().route(
            "/api/graphrag/v1/ontology/subgraph",
            get(|| async {
                Json(json!({
                    "nodes": [
                        {
                            "id": "net_revenue",
                            "label": "BusinessMetric",
                            "layer": 2,
                            "description": "Net revenue in USD",
                            "epistemic_status": "verified",
                            "confidence": 1.0,
                            "verified_by": "steward_alice",
                            "properties": { "formula": "SUM(order_total)" }
                        }
                    ],
                    "edges": [
                        {
                            "source_id": "net_revenue",
                            "predicate": "COMPUTED_FROM",
                            "target_id": "wave1_orders",
                            "epistemic_status": "verified",
                            "confidence": 1.0,
                            "properties": {}
                        }
                    ],
                    "communities": [
                        {
                            "id": "comm_revenue",
                            "label": "Revenue & Billing",
                            "member_node_ids": ["net_revenue", "wave1_orders"]
                        }
                    ]
                }))
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_app).await;
        });

        let dir = tempdir().unwrap();
        seed_validation_suite_with_options(42, 40, dir.path(), false).unwrap();
        let orders_pq = dir.path().join("sales_orders.parquet");
        let prof = profile_parquet_dataset("wave1_orders", &orders_pq, &sample_profiler_cfg(), None)
            .unwrap();

        let catalog = TopologyCatalog::default();
        catalog
            .upsert_dataset("s3://lake", "1 row per order", Some(orders_pq.to_string_lossy().to_string()), prof)
            .unwrap();

        let cfg = VisualsConfig {
            graphrag_base_url: Some(
                url::Url::parse(&format!("http://{}/api/graphrag/v1", addr)).unwrap(),
            ),
            ..Default::default()
        };
        let graph = build_topology_graph(
            &catalog,
            &FieldStatisticalCatalog::default(),
            &ProvenanceStore::default(),
            &cfg,
            &TopologyGraphQuery::default(),
        )
        .await
        .expect("fused topology graph");

        server_handle.abort();

        assert!(!graph.graphrag_overlay_degraded);
        assert!(graph.nodes.iter().any(|n| n.id == "net_revenue" && n.layer == 2));
        assert!(graph.nodes.iter().any(|n| n.id == "wave1_orders" && n.layer == 1));
        assert!(graph.edges.iter().any(|e| {
            e.source_id == "net_revenue"
                && e.predicate == "COMPUTED_FROM"
                && e.target_id == "wave1_orders"
        }));
        assert_eq!(graph.communities.len(), 1);
    }

    #[tokio::test]
    async fn test_topology_graph_degrades_gracefully_when_graphrag_unreachable() {
        let dir = tempdir().unwrap();
        seed_validation_suite_with_options(42, 30, dir.path(), false).unwrap();
        let orders_pq = dir.path().join("sales_orders.parquet");
        let prof = profile_parquet_dataset("wave1_orders", &orders_pq, &sample_profiler_cfg(), None)
            .unwrap();

        let catalog = TopologyCatalog::default();
        catalog
            .upsert_dataset("s3://lake", "1 row per order", None, prof)
            .unwrap();

        let cfg = VisualsConfig {
            graphrag_base_url: Some(
                url::Url::parse("http://127.0.0.1:1/api/graphrag/v1").unwrap(),
            ),
            ..Default::default()
        };
        let graph = build_topology_graph(
            &catalog,
            &FieldStatisticalCatalog::default(),
            &ProvenanceStore::default(),
            &cfg,
            &TopologyGraphQuery::default(),
        )
        .await
        .expect("degraded topology graph must still succeed");

        assert!(graph.graphrag_overlay_degraded);
        assert!(graph.nodes.iter().any(|n| n.id == "wave1_orders" && n.layer == 1));
    }

    #[tokio::test]
    async fn test_topology_graph_annotates_trap_01_forbidden_trap_03_fanout_and_trap_04_drift() {
        let dir = tempdir().unwrap();
        seed_validation_suite_with_options(42, 40, dir.path(), false).unwrap();
        let p_cfg = sample_profiler_cfg();
        let c_cfg = sample_compute_cfg();

        let orders = profile_parquet_dataset("orders", &dir.path().join("sales_orders.parquet"), &p_cfg, None).unwrap();
        let w1_pay = profile_parquet_dataset("payment_events", &dir.path().join("payment_events_wave1.parquet"), &p_cfg, None).unwrap();
        let w3_pay = profile_parquet_dataset("payment_events", &dir.path().join("payment_events_wave3.parquet"), &p_cfg, Some(&w1_pay)).unwrap();
        let items = profile_parquet_dataset("order_items", &dir.path().join("order_items.parquet"), &p_cfg, None).unwrap();
        let payments = profile_parquet_dataset("order_payments", &dir.path().join("order_payments.parquet"), &p_cfg, None).unwrap();
        let billing = profile_parquet_dataset("billing_subscriptions", &dir.path().join("billing_subscriptions.parquet"), &p_cfg, None).unwrap();
        let leads = profile_parquet_dataset("marketing_leads", &dir.path().join("marketing_leads.parquet"), &p_cfg, None).unwrap();

        let catalog = TopologyCatalog::default();
        catalog.upsert_dataset("s3://lake", "1 row per order", None, orders).unwrap();
        catalog.upsert_dataset("s3://lake", "1 row per payment event", None, w3_pay).unwrap();
        catalog.upsert_dataset("s3://lake", "1 row per item", None, items).unwrap();
        catalog.upsert_dataset("s3://lake", "1 row per payment", None, payments).unwrap();
        catalog.upsert_dataset("postgres://billing", "1 row per sub", None, billing).unwrap();
        catalog.upsert_dataset("postgres://mkt", "1 row per lead", None, leads).unwrap();

        // TRAP-01: FORBIDDEN_JOIN
        let trap1_probe = probe_join(
            "billing_subscriptions",
            &dir.path().join("billing_subscriptions.parquet"),
            "account_id",
            "marketing_leads",
            &dir.path().join("marketing_leads.parquet"),
            "account_id",
            &c_cfg,
        )
        .unwrap();
        let trap1_rec = catalog.upsert_join_probe(trap1_probe, None, None).unwrap();
        catalog
            .steward_join(
                &trap1_rec.id,
                JoinStewardUpdateRequest {
                    epistemic_status: "rejected".to_string(),
                    verified_by: Some("steward_alice".to_string()),
                    steward_notes: Some("Homonym ID namespaces".to_string()),
                    filter_invariant: None,
                },
            )
            .unwrap();

        // TRAP-03: Fan-out (orders -> order_items & orders -> order_payments)
        let p_items = probe_join(
            "orders",
            &dir.path().join("sales_orders.parquet"),
            "order_id",
            "order_items",
            &dir.path().join("order_items.parquet"),
            "order_id",
            &c_cfg,
        )
        .unwrap();
        let p_pay = probe_join(
            "orders",
            &dir.path().join("sales_orders.parquet"),
            "order_id",
            "order_payments",
            &dir.path().join("order_payments.parquet"),
            "order_id",
            &c_cfg,
        )
        .unwrap();
        catalog.upsert_join_probe(p_items, None, None).unwrap();
        catalog.upsert_join_probe(p_pay, None, None).unwrap();

        let graph = build_topology_graph(
            &catalog,
            &FieldStatisticalCatalog::default(),
            &ProvenanceStore::default(),
            &VisualsConfig::default(),
            &TopologyGraphQuery::default(),
        )
        .await
        .expect("trap topology");

        // TRAP-01: FORBIDDEN_JOIN edge
        assert!(graph.edges.iter().any(|e| {
            e.predicate == "FORBIDDEN_JOIN"
                && e.source_id == "billing_subscriptions.account_id"
                && e.target_id == "marketing_leads.account_id"
        }));
        // TRAP-03: fan_out_warning annotated
        assert!(graph.edges.iter().any(|e| {
            e.predicate == "JOINABLE_WITH"
                && e.source_id == "orders.order_id"
                && e.fan_out_warning.is_some()
        }));
        // TRAP-04: drifted badge on payment_events
        assert!(graph.nodes.iter().any(|n| {
            n.id == "payment_events"
                && n.epistemic_status == "drifted"
                && n.badges.iter().any(|b| b == "drifted")
        }));
    }

    #[test]
    fn test_dataset_anatomy_builds_nested_arrow_schema_tree() {
        let profile = DatasetProfile {
            dataset_name: "events_nested".to_string(),
            row_count: 100,
            schema_fingerprint_sha256: "sha_nested".to_string(),
            candidate_primary_keys: vec!["event_id".to_string()],
            epistemic_status: "verified".to_string(),
            drift_findings: vec![],
            fields: vec![
                FieldProfile {
                    field_name: "customer.address.zip".to_string(),
                    arrow_data_type: "Utf8".to_string(),
                    null_count: 0,
                    null_ratio: 0.0,
                    distinct_count: 50,
                    uniqueness_ratio: 0.5,
                    top_k_values: vec!["94107".to_string()],
                    dominant_prefix: None,
                    minhash_signature: vec![],
                    numeric_stats: None,
                    inferred_semantic_domain: "categorical".to_string(),
                    epistemic_status: "verified".to_string(),
                },
                FieldProfile {
                    field_name: "items[].sku".to_string(),
                    arrow_data_type: "Utf8".to_string(),
                    null_count: 0,
                    null_ratio: 0.0,
                    distinct_count: 20,
                    uniqueness_ratio: 0.2,
                    top_k_values: vec!["SKU-01".to_string()],
                    dominant_prefix: Some("SKU".to_string()),
                    minhash_signature: vec![],
                    numeric_stats: None,
                    inferred_semantic_domain: "identifier".to_string(),
                    epistemic_status: "verified".to_string(),
                },
            ],
        };
        let catalog = TopologyCatalog::default();
        catalog
            .upsert_dataset("mongodb://events", "1 doc per event", None, profile)
            .unwrap();

        let anatomy =
            build_dataset_anatomy("events_nested", &catalog, &FieldStatisticalCatalog::default())
                .expect("nested anatomy");

        let customer = anatomy
            .schema_tree
            .iter()
            .find(|n| n.name == "customer" && n.node_kind == "struct")
            .expect("customer struct node");
        let address = customer
            .children
            .iter()
            .find(|n| n.name == "address" && n.node_kind == "struct")
            .expect("address struct node");
        assert!(address.children.iter().any(|n| n.name == "zip" && n.node_kind == "field"));

        let items = anatomy
            .schema_tree
            .iter()
            .find(|n| n.name == "items[]" && n.node_kind == "list")
            .expect("items[] list node");
        assert!(items.children.iter().any(|n| n.name == "sku" && n.node_kind == "field"));
    }

    #[test]
    fn test_join_anatomy_computes_type_compatibility_overlap_strip_and_fanout() {
        let catalog = TopologyCatalog::default();
        catalog
            .upsert_dataset(
                "s3://lake",
                "orders",
                None,
                DatasetProfile {
                    dataset_name: "orders".to_string(),
                    row_count: 100,
                    schema_fingerprint_sha256: "s1".to_string(),
                    candidate_primary_keys: vec![],
                    epistemic_status: "verified".to_string(),
                    drift_findings: vec![],
                    fields: vec![FieldProfile {
                        field_name: "cust_id".to_string(),
                        arrow_data_type: "Int64".to_string(),
                        null_count: 0,
                        null_ratio: 0.0,
                        distinct_count: 50,
                        uniqueness_ratio: 0.5,
                        top_k_values: vec![],
                        dominant_prefix: None,
                        minhash_signature: vec![],
                        numeric_stats: None,
                        inferred_semantic_domain: "identifier".to_string(),
                        epistemic_status: "verified".to_string(),
                    }],
                },
            )
            .unwrap();
        catalog
            .upsert_dataset(
                "postgres://crm",
                "customers",
                None,
                DatasetProfile {
                    dataset_name: "customers".to_string(),
                    row_count: 60,
                    schema_fingerprint_sha256: "s2".to_string(),
                    candidate_primary_keys: vec!["id".to_string()],
                    epistemic_status: "verified".to_string(),
                    drift_findings: vec![],
                    fields: vec![FieldProfile {
                        field_name: "id".to_string(),
                        arrow_data_type: "Utf8".to_string(),
                        null_count: 0,
                        null_ratio: 0.0,
                        distinct_count: 60,
                        uniqueness_ratio: 1.0,
                        top_k_values: vec![],
                        dominant_prefix: None,
                        minhash_signature: vec![],
                        numeric_stats: None,
                        inferred_semantic_domain: "identifier".to_string(),
                        epistemic_status: "verified".to_string(),
                    }],
                },
            )
            .unwrap();

        let rec = catalog
            .upsert_join_probe(
                JoinProbeResult {
                    left_dataset: "orders".to_string(),
                    left_field: "cust_id".to_string(),
                    right_dataset: "customers".to_string(),
                    right_field: "id".to_string(),
                    left_row_count: 100,
                    right_row_count: 60,
                    left_distinct_count: 50,
                    right_distinct_count: 60,
                    joined_row_count: 90,
                    inclusion_ratio_left_in_right: 0.90,
                    inclusion_ratio_right_in_left: 0.75,
                    jaccard_similarity: 0.69,
                    observed_cardinality: "N:1".to_string(),
                    row_multiplication_factor: 1.0,
                    fan_out_risk: false,
                    homonym_trap_warning: false,
                    confidence: 0.92,
                    epistemic_status: "empirically_tested".to_string(),
                    recommended_action: "verify".to_string(),
                    explanation: "90% left inclusion".to_string(),
                },
                None,
                None,
            )
            .unwrap();

        let anatomy = build_join_anatomy(&rec.id, &catalog).expect("join anatomy");
        assert!(!anatomy.type_compatibility.compatible);
        assert!(anatomy.type_compatibility.cast_required.is_some());
        assert_eq!(anatomy.overlap_strip.matched_distinct_count, 45);
        assert_eq!(anatomy.overlap_strip.left_orphan_count, 5);
        assert_eq!(anatomy.overlap_strip.right_orphan_count, 15);
        assert!(anatomy.explore_sql_template.contains("JOIN customers"));
    }

    #[tokio::test]
    async fn test_topology_mermaid_emits_valid_er_diagram_and_layered_flowchart() {
        let dir = tempdir().unwrap();
        seed_validation_suite_with_options(42, 30, dir.path(), false).unwrap();
        let prof = profile_parquet_dataset(
            "orders",
            &dir.path().join("sales_orders.parquet"),
            &sample_profiler_cfg(),
            None,
        )
        .unwrap();
        let catalog = TopologyCatalog::default();
        catalog
            .upsert_dataset("s3://lake", "1 row per order", None, prof)
            .unwrap();

        let er_graph = build_topology_graph(
            &catalog,
            &FieldStatisticalCatalog::default(),
            &ProvenanceStore::default(),
            &VisualsConfig::default(),
            &TopologyGraphQuery {
                lens: Some("er_join".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let er_mm = export_topology_mermaid(er_graph);
        assert_eq!(er_mm.diagram_kind, "erDiagram");
        assert!(er_mm.mermaid.starts_with("erDiagram"));
        assert!(er_mm.mermaid.contains("ORDERS {"));

        let layered_graph = build_topology_graph(
            &catalog,
            &FieldStatisticalCatalog::default(),
            &ProvenanceStore::default(),
            &VisualsConfig::default(),
            &TopologyGraphQuery {
                lens: Some("semantic_3layer".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let layered_mm = export_topology_mermaid(layered_graph);
        assert_eq!(layered_mm.diagram_kind, "flowchart");
        assert!(layered_mm.mermaid.starts_with("flowchart TB"));
        assert!(layered_mm.mermaid.contains("Layer 1: Datasets"));
    }
}
