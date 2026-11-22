import { Injectable, inject } from '@angular/core';
import { HttpClient, HttpParams } from '@angular/common/http';
import { firstValueFrom } from 'rxjs';
import { ConfigService } from './config.service';

export interface TrustSummary {
  all_rules_human_verified: boolean;
  min_rule_confidence: number;
  verified_by_stewards: string[];
  pre_aggregation_applied: boolean;
  unit_conversion_applied: boolean;
}

export interface DataBinding {
  kind: string;
  dataset_id?: string | null;
  parquet_path?: string | null;
  sql?: string | null;
  algorithm_id?: string | null;
  group_by?: string | null;
  metric_column?: string | null;
  aggregation?: string | null;
}

export interface ReferenceLine {
  axis: string;
  value: number;
  label: string;
  style?: string;
}

export interface VisualAnnotation {
  x: any;
  label: string;
  severity?: string;
}

export interface ProfileOverlays {
  reference_lines: ReferenceLine[];
  annotations: VisualAnnotation[];
  log_scale_recommended?: boolean;
  seasonality_period?: string | null;
}

export interface DataVisualSpec {
  visual_id: string;
  title: string;
  description: string;
  renderer: 'vega_lite' | 'echarts';
  chart_family: string;
  data_binding: DataBinding;
  spec: Record<string, any>;
  profile_overlays?: ProfileOverlays;
}

export interface VisualProvenance {
  provenance_run_id: string;
  trust_summary: TrustSummary;
  sampled: boolean;
  population_rows: number;
  rendered_rows: number;
  sampling_seed?: number | null;
}

export interface ChartRecommendation {
  rank: number;
  score: number;
  rationale: string;
  visual_spec: DataVisualSpec;
}

export interface VisualRecommendResponse {
  dataset_name: string;
  recommendations: ChartRecommendation[];
}

export interface VisualQueryResponse {
  visual_spec: DataVisualSpec;
  data_values: Record<string, any>[];
  provenance: VisualProvenance;
}

export interface TopologyCommunity {
  id: string;
  label: string;
  member_node_ids: string[];
}

export interface TopologyNode {
  id: string;
  kind: 'source' | 'dataset' | 'field' | 'business_concept' | 'business_metric' | 'community_summary' | string;
  layer: number;
  parent_id?: string | null;
  label: string;
  subtitle?: string | null;
  epistemic_status: string;
  confidence: number;
  badges: string[];
  field_count?: number | null;
  metadata: Record<string, any>;
}

export interface FanOutWarning {
  multiplier_estimate: number;
  reason: string;
  suggested_cte_pattern: string;
}

export interface TopologyEdge {
  id: string;
  source_id: string;
  target_id: string;
  predicate: string;
  cardinality?: string | null;
  epistemic_status: string;
  confidence: number;
  inclusion_ratio?: number | null;
  fan_out_warning?: FanOutWarning | null;
  filter_invariant?: string | null;
  steward_notes?: string | null;
}

export interface HighlightedPath {
  node_ids: string[];
  edge_ids: string[];
  all_rules_human_verified: boolean;
  fan_out_warnings: string[];
}

export interface SemanticTopologyGraph {
  lens: 'semantic_3layer' | 'er_join' | 'field_anatomy' | 'lineage_overlay' | string;
  graphrag_overlay_degraded: boolean;
  truncated: boolean;
  total_nodes: number;
  total_edges: number;
  applied_filter_hint?: string | null;
  communities: TopologyCommunity[];
  nodes: TopologyNode[];
  edges: TopologyEdge[];
  highlighted_path?: HighlightedPath | null;
}

export interface SchemaTreeNode {
  name: string;
  full_path: string;
  node_kind: 'struct' | 'list' | 'field';
  arrow_data_type?: string | null;
  is_primary_key_candidate: boolean;
  epistemic_status?: string | null;
  null_ratio?: number | null;
  distinct_count?: number | null;
  top_values: string[];
  mini_histogram_bins: { label: string; count: number }[];
  statistical_profile?: Record<string, any> | null;
  children: SchemaTreeNode[];
}

export interface DatasetAnatomyResponse {
  dataset_id: string;
  canonical_id: string;
  format: string;
  grain_description: string;
  row_count_estimate: number;
  schema_fingerprint_sha256: string;
  epistemic_status: string;
  schema_tree: SchemaTreeNode[];
  incident_joins: any[];
  semantic_links: TopologyNode[];
}

export interface SchemaCardField {
  field_name: string;
  arrow_data_type: string;
  is_join_key: boolean;
  is_primary_key_candidate: boolean;
  null_ratio: number;
  distinct_count: number;
  epistemic_status: string;
}

export interface SchemaCard {
  dataset_id: string;
  row_count: number;
  join_field: string;
  fields: SchemaCardField[];
}

export interface JoinAnatomyResponse {
  join: {
    id: string;
    predicate: string;
    epistemic_status: string;
    verified_by?: string | null;
    steward_notes?: string | null;
    filter_invariant?: string | null;
    probe: Record<string, any>;
  };
  left_dataset: SchemaCard;
  right_dataset: SchemaCard;
  type_compatibility: {
    compatible: boolean;
    left_arrow_type: string;
    right_arrow_type: string;
    cast_required?: string | null;
    warning?: string | null;
  };
  overlap_strip: {
    jaccard_overlap: number;
    left_inclusion_ratio: number;
    right_inclusion_ratio: number;
    left_distinct_count: number;
    right_distinct_count: number;
    matched_distinct_count: number;
    left_orphan_count: number;
    right_orphan_count: number;
  };
  matched_samples: string[];
  left_orphan_samples: string[];
  right_orphan_samples: string[];
  fan_out_diagnostics: {
    is_fan_out_risk: boolean;
    row_multiplier_estimate: number;
    explanation: string;
    safe_cte_sql_hint?: string | null;
  };
  explore_sql_template: string;
}

@Injectable({ providedIn: 'root' })
export class DataEngineApiService {
  private readonly http = inject(HttpClient);
  private readonly config = inject(ConfigService);

  private async baseUrl(): Promise<string> {
    const cfg = await this.config.loadConfig();
    return cfg.apiUrl;
  }

  async recommendVisuals(datasetName: string, renderer: 'vega_lite' | 'echarts' = 'vega_lite'): Promise<VisualRecommendResponse> {
    const api = await this.baseUrl();
    return firstValueFrom(
      this.http.post<VisualRecommendResponse>(`${api}/visuals/recommend`, {
        dataset_name: datasetName,
        renderer,
      }),
    );
  }

  async queryVisual(payload: Record<string, any>): Promise<VisualQueryResponse> {
    const api = await this.baseUrl();
    return firstValueFrom(this.http.post<VisualQueryResponse>(`${api}/visuals/query`, payload));
  }

  async saveVisual(payload: Record<string, any>): Promise<any> {
    const api = await this.baseUrl();
    return firstValueFrom(this.http.post(`${api}/visuals`, payload));
  }

  async getTopologyGraph(params: Record<string, string | number | boolean | undefined> = {}): Promise<SemanticTopologyGraph> {
    const api = await this.baseUrl();
    let httpParams = new HttpParams();
    for (const [key, value] of Object.entries(params)) {
      if (value !== undefined && value !== null && value !== '') {
        httpParams = httpParams.set(key, String(value));
      }
    }
    return firstValueFrom(this.http.get<SemanticTopologyGraph>(`${api}/topology/graph`, { params: httpParams }));
  }

  async getDatasetAnatomy(datasetId: string): Promise<DatasetAnatomyResponse> {
    const api = await this.baseUrl();
    return firstValueFrom(
      this.http.get<DatasetAnatomyResponse>(`${api}/topology/anatomy/dataset/${encodeURIComponent(datasetId)}`),
    );
  }

  async getJoinAnatomy(joinId: string): Promise<JoinAnatomyResponse> {
    const api = await this.baseUrl();
    return firstValueFrom(
      this.http.get<JoinAnatomyResponse>(`${api}/topology/anatomy/join/${encodeURIComponent(joinId)}`),
    );
  }

  async stewardJoin(
    joinId: string,
    payload: { epistemic_status: string; verified_by?: string; steward_notes?: string; filter_invariant?: string },
  ): Promise<any> {
    const api = await this.baseUrl();
    return firstValueFrom(
      this.http.put(`${api}/join-hypotheses/${encodeURIComponent(joinId)}`, payload),
    );
  }

  async verifyStatisticalField(
    canonicalFieldId: string,
    payload: { verified_by: string; recommended_test_family?: string; known_confounders?: string[] },
  ): Promise<any> {
    const api = await this.baseUrl();
    return firstValueFrom(
      this.http.put(`${api}/fields/${encodeURIComponent(canonicalFieldId)}`, payload),
    );
  }
}
