import {
  AfterViewInit,
  Component,
  ElementRef,
  Input,
  OnDestroy,
  ViewChild,
  inject,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import cytoscape, { Core as CytoscapeCore, ElementDefinition } from 'cytoscape';
import elk from 'cytoscape-elk';

import {
  AiFieldAttribution,
  CanvasManagedComponent,
  JsonPatchOp,
  MfeMountProps,
  applyJsonPatches,
} from '../mfe-canvas.types';
import {
  DataEngineApiService,
  DatasetAnatomyResponse,
  JoinAnatomyResponse,
  SemanticTopologyGraph,
  TopologyEdge,
  TopologyNode,
} from '../../services/data-engine-api.service';

let elkRegistered = false;
function ensureElkRegistered(): void {
  if (!elkRegistered) {
    try {
      cytoscape.use(elk);
      elkRegistered = true;
    } catch {
      // Safe no-op if already registered
    }
  }
}

export type TopologyLens = 'semantic_3layer' | 'er_join' | 'field_anatomy' | 'lineage_overlay';

export interface SemanticTopologyState {
  lens: TopologyLens;
  visible_layers: number[];
  epistemic_filters: string[];
  focus_node_id: string | null;
  expanded_datasets: string[];
  highlighted_path: {
    node_ids: string[];
    edge_ids: string[];
    all_rules_human_verified?: boolean;
    fan_out_warnings?: string[];
  } | null;
  graph: SemanticTopologyGraph;
  active_dataset_anatomy: DatasetAnatomyResponse | null;
  active_join_anatomy: JoinAnatomyResponse | null;
}

export function defaultSemanticTopologyState(): SemanticTopologyState {
  return {
    lens: 'semantic_3layer',
    visible_layers: [0, 1, 2],
    epistemic_filters: [],
    focus_node_id: null,
    expanded_datasets: ['sales_orders', 'crm_customers'],
    highlighted_path: null,
    graph: {
      lens: 'semantic_3layer',
      graphrag_overlay_degraded: false,
      truncated: false,
      total_nodes: 7,
      total_edges: 7,
      applied_filter_hint: null,
      communities: [
        {
          id: 'comm_revenue',
          label: 'Revenue & Billing',
          member_node_ids: ['net_revenue', 'sales_orders', 'crm_customers'],
        },
      ],
      nodes: [
        {
          id: 'net_revenue',
          kind: 'business_metric',
          layer: 2,
          label: 'Net Revenue',
          subtitle: 'SUM(amount_usd) - discount',
          epistemic_status: 'verified',
          confidence: 1.0,
          badges: ['BusinessMetric'],
          metadata: {},
        },
        {
          id: 'sales_orders',
          kind: 'dataset',
          layer: 1,
          label: 'sales_orders',
          subtitle: '1 row/order • 48,200 rows',
          epistemic_status: 'drifted',
          confidence: 0.92,
          badges: ['parquet', 'drifted'],
          field_count: 2,
          metadata: {},
        },
        {
          id: 'crm_customers',
          kind: 'dataset',
          layer: 1,
          label: 'crm_customers',
          subtitle: '1 row/account • 12,100 rows',
          epistemic_status: 'verified',
          confidence: 1.0,
          badges: ['postgres', 'verified'],
          field_count: 2,
          metadata: {},
        },
        {
          id: 'sales_orders.cust_ref_key',
          kind: 'field',
          layer: 0,
          parent_id: 'sales_orders',
          label: 'cust_ref_key',
          subtitle: 'Utf8 • 0% null',
          epistemic_status: 'empirically_tested',
          confidence: 0.97,
          badges: ['Utf8'],
          metadata: {},
        },
        {
          id: 'sales_orders.amount_usd',
          kind: 'field',
          layer: 0,
          parent_id: 'sales_orders',
          label: 'amount_usd',
          subtitle: 'Float64 • DRIFTED',
          epistemic_status: 'drifted',
          confidence: 0.91,
          badges: ['Float64', 'heavy_tail', 'drifted'],
          metadata: {},
        },
        {
          id: 'crm_customers.party_uuid',
          kind: 'field',
          layer: 0,
          parent_id: 'crm_customers',
          label: 'party_uuid',
          subtitle: 'Utf8 • PK',
          epistemic_status: 'verified',
          confidence: 1.0,
          badges: ['Utf8', 'pk'],
          metadata: {},
        },
        {
          id: 'crm_customers.segment',
          kind: 'field',
          layer: 0,
          parent_id: 'crm_customers',
          label: 'segment',
          subtitle: 'Utf8 • CONFOUNDER',
          epistemic_status: 'verified',
          confidence: 1.0,
          badges: ['Utf8'],
          metadata: {},
        },
      ],
      edges: [
        {
          id: 'e_metric_orders',
          source_id: 'net_revenue',
          target_id: 'sales_orders.amount_usd',
          predicate: 'COMPUTED_FROM',
          epistemic_status: 'verified',
          confidence: 1.0,
        },
        {
          id: 'join_sales_orders_cust_ref_key__crm_customers_party_uuid',
          source_id: 'sales_orders.cust_ref_key',
          target_id: 'crm_customers.party_uuid',
          predicate: 'JOINABLE_WITH',
          cardinality: 'N:1',
          epistemic_status: 'empirically_tested',
          confidence: 0.97,
          inclusion_ratio: 0.97,
        },
        {
          id: 'e_confounder',
          source_id: 'sales_orders.amount_usd',
          target_id: 'crm_customers.segment',
          predicate: 'CONFOUNDED_BY',
          epistemic_status: 'empirically_tested',
          confidence: 0.93,
          steward_notes: 'Simpson paradox confounder',
        },
      ],
    },
    active_dataset_anatomy: null,
    active_join_anatomy: {
      join: {
        id: 'join_sales_orders_cust_ref_key__crm_customers_party_uuid',
        predicate: 'JOINABLE_WITH',
        epistemic_status: 'empirically_tested',
        verified_by: null,
        steward_notes: null,
        filter_invariant: null,
        probe: { observed_cardinality: 'N:1', inclusion_ratio_left_in_right: 0.97 },
      },
      left_dataset: {
        dataset_id: 'sales_orders',
        row_count: 48200,
        join_field: 'cust_ref_key',
        fields: [
          { field_name: 'order_id', arrow_data_type: 'Int64', is_join_key: false, is_primary_key_candidate: true, null_ratio: 0, distinct_count: 48200, epistemic_status: 'verified' },
          { field_name: 'cust_ref_key', arrow_data_type: 'Utf8', is_join_key: true, is_primary_key_candidate: false, null_ratio: 0, distinct_count: 11737, epistemic_status: 'empirically_tested' },
          { field_name: 'amount_usd', arrow_data_type: 'Float64', is_join_key: false, is_primary_key_candidate: false, null_ratio: 0, distinct_count: 9120, epistemic_status: 'drifted' },
        ],
      },
      right_dataset: {
        dataset_id: 'crm_customers',
        row_count: 12100,
        join_field: 'party_uuid',
        fields: [
          { field_name: 'party_uuid', arrow_data_type: 'Utf8', is_join_key: true, is_primary_key_candidate: true, null_ratio: 0, distinct_count: 12100, epistemic_status: 'verified' },
          { field_name: 'segment', arrow_data_type: 'Utf8', is_join_key: false, is_primary_key_candidate: false, null_ratio: 0, distinct_count: 3, epistemic_status: 'verified' },
        ],
      },
      type_compatibility: {
        compatible: true,
        left_arrow_type: 'Utf8',
        right_arrow_type: 'Utf8',
        cast_required: null,
        warning: null,
      },
      overlap_strip: {
        jaccard_overlap: 0.94,
        left_inclusion_ratio: 0.97,
        right_inclusion_ratio: 0.97,
        left_distinct_count: 11737,
        right_distinct_count: 12100,
        matched_distinct_count: 11385,
        left_orphan_count: 352,
        right_orphan_count: 715,
      },
      matched_samples: ['CUST-0001 -> CUST-0001'],
      left_orphan_samples: ['CUST-ORPHAN-99'],
      right_orphan_samples: ['CUST-INACTIVE-12'],
      fan_out_diagnostics: {
        is_fan_out_risk: false,
        row_multiplier_estimate: 1.0,
        explanation: 'Safe N:1 join — right key party_uuid is unique.',
        safe_cte_sql_hint: null,
      },
      explore_sql_template:
        'SELECT l.*, r.* FROM sales_orders l JOIN crm_customers r ON l.cust_ref_key = r.party_uuid LIMIT 1000',
    },
  };
}

export function buildCytoscapeElements(state: SemanticTopologyState): ElementDefinition[] {
  const elements: ElementDefinition[] = [];
  const allowedLayers = new Set(state.visible_layers);
  const statusFilters = new Set(state.epistemic_filters.map((s) => s.toLowerCase()));
  const datasetNodes = state.graph.nodes.filter((n) => n.kind === 'dataset');
  const autoCollapseFields = datasetNodes.length > 15;
  const expandedSet = new Set(state.expanded_datasets);
  const highlightedNodes = new Set(state.highlighted_path?.node_ids ?? []);
  const highlightedEdges = new Set(state.highlighted_path?.edge_ids ?? []);
  const isLineageLens = state.lens === 'lineage_overlay' && highlightedNodes.size > 0;

  const keptIds = new Set<string>();
  for (const node of state.graph.nodes) {
    if (!allowedLayers.has(node.layer)) continue;
    if (state.lens === 'er_join' && node.layer === 2) continue;
    if (
      statusFilters.size > 0 &&
      node.kind !== 'source' &&
      !statusFilters.has(node.epistemic_status.toLowerCase())
    ) {
      continue;
    }
    if (
      node.kind === 'field' &&
      autoCollapseFields &&
      node.parent_id &&
      !expandedSet.has(node.parent_id)
    ) {
      continue;
    }

    keptIds.add(node.id);
    const classes: string[] = [
      `kind-${node.kind}`,
      `layer-${node.layer}`,
      `status-${node.epistemic_status}`,
    ];
    if (node.badges.includes('drifted') || node.epistemic_status === 'drifted') {
      classes.push('is-drifted');
    }
    if (isLineageLens && !highlightedNodes.has(node.id)) {
      classes.push('is-dimmed');
    }
    if (highlightedNodes.has(node.id)) {
      classes.push('is-highlighted');
    }

    elements.push({
      data: {
        id: node.id,
        label: node.label,
        parent: node.parent_id && keptIds.has(node.parent_id) ? node.parent_id : undefined,
        kind: node.kind,
        layer: node.layer,
        epistemic_status: node.epistemic_status,
      },
      classes: classes.join(' '),
    });
  }

  for (const edge of state.graph.edges) {
    if (!keptIds.has(edge.source_id) || !keptIds.has(edge.target_id)) continue;
    const classes: string[] = [`pred-${edge.predicate}`, `status-${edge.epistemic_status}`];
    if (edge.predicate === 'FORBIDDEN_JOIN') classes.push('is-forbidden');
    if (edge.predicate === 'CONFOUNDED_BY') classes.push('is-confounder');
    if (edge.fan_out_warning) classes.push('has-fanout');
    if (isLineageLens && !highlightedEdges.has(edge.id)) classes.push('is-dimmed');
    if (highlightedEdges.has(edge.id)) classes.push('is-highlighted');

    const edgeLabel = edge.fan_out_warning
      ? `⚠ ${edge.fan_out_warning.multiplier_estimate.toFixed(2)}x ${edge.cardinality ?? ''}`
      : edge.predicate === 'FORBIDDEN_JOIN'
        ? '✕ FORBIDDEN_JOIN'
        : edge.cardinality
          ? `${edge.cardinality} (${Math.round((edge.inclusion_ratio ?? 1) * 100)}%)`
          : edge.predicate;

    elements.push({
      data: {
        id: edge.id,
        source: edge.source_id,
        target: edge.target_id,
        label: edgeLabel,
        predicate: edge.predicate,
        epistemic_status: edge.epistemic_status,
      },
      classes: classes.join(' '),
    });
  }

  return elements;
}

@Component({
  selector: 'de-semantic-topology',
  standalone: true,
  imports: [CommonModule, FormsModule],
  template: `
    <div class="topo-shell" data-testid="semantic-topology">
      <header class="topo-header">
        <div class="lens-tabs" data-testid="lens-switcher">
          <button type="button" class="tab" [class.active]="state.lens === 'semantic_3layer'" data-testid="lens-semantic_3layer" (click)="setLens('semantic_3layer')">
            3-Layer Semantic
          </button>
          <button type="button" class="tab" [class.active]="state.lens === 'er_join'" data-testid="lens-er_join" (click)="setLens('er_join')">
            ER &amp; Joins
          </button>
          <button type="button" class="tab" [class.active]="state.lens === 'field_anatomy'" data-testid="lens-field_anatomy" (click)="setLens('field_anatomy')">
            Field Anatomy
          </button>
          <button type="button" class="tab" [class.active]="state.lens === 'lineage_overlay'" data-testid="lens-lineage_overlay" (click)="setLens('lineage_overlay')">
            Lineage Overlay
          </button>
        </div>
        <div class="filter-chips">
          <span class="de-badge de-badge-verified">Nodes: {{ visibleNodes.length }}</span>
          <span class="de-badge de-badge-tested">Edges: {{ visibleEdges.length }}</span>
          <span *ngIf="selectedLabel" class="de-badge de-badge-tested" data-testid="topo-selection-chip">
            🎯 {{ selectedLabel }}
          </span>
        </div>
      </header>

      <div *ngIf="state.graph.graphrag_overlay_degraded" class="degraded-banner" data-testid="degraded-overlay-banner">
        Showing physical catalog topology; GraphRAG Layer 2 semantic overlay is temporarily unavailable.
      </div>

      <div class="topo-body" [class.anatomy-mode]="state.lens === 'field_anatomy'">
        <div class="graph-pane">
          <div #cyHost class="cy-host" data-testid="cy-host"></div>

          <div class="interactive-index" data-testid="topology-element-list">
            <div class="index-group">
              <h4>Relationships &amp; Structural Edges</h4>
              <button
                *ngFor="let edge of visibleEdges"
                type="button"
                class="edge-row"
                [attr.data-testid]="'edge-' + edge.id"
                [ngClass]="edgeCssClass(edge)"
                (click)="selectEdge(edge)">
                <span class="edge-endpoints">{{ edge.source_id }} → {{ edge.target_id }}</span>
                <span class="edge-meta">
                  <span class="de-badge" [ngClass]="statusBadgeClass(edge.epistemic_status, edge.predicate)">
                    {{ edge.predicate }}{{ edge.cardinality ? ' ' + edge.cardinality : '' }}
                  </span>
                  <span *ngIf="edge.fan_out_warning" class="de-badge de-badge-warning" data-testid="fanout-badge">
                    ⚠ {{ edge.fan_out_warning.multiplier_estimate | number:'1.1-2' }}x fan-out
                  </span>
                </span>
              </button>
            </div>
          </div>
        </div>

        <aside *ngIf="state.lens === 'field_anatomy' || state.active_join_anatomy || state.active_dataset_anatomy" class="anatomy-pane" data-testid="field-anatomy-panel">
          <ng-container *ngIf="state.active_join_anatomy as ja">
            <div class="anatomy-head">
              <h3>Field-to-Field Join Anatomy</h3>
              <span class="de-badge" [ngClass]="statusBadgeClass(ja.join.epistemic_status, ja.join.predicate)">
                {{ ja.join.predicate }} ({{ ja.join.epistemic_status }})
              </span>
            </div>

            <div class="schema-pair">
              <div class="schema-card">
                <div class="card-title">{{ ja.left_dataset.dataset_id }} ({{ ja.left_dataset.row_count | number }} rows)</div>
                <div *ngFor="let f of ja.left_dataset.fields" class="field-row" [class.join-key]="f.is_join_key">
                  <span>{{ f.field_name }}</span>
                  <span class="type-pill">{{ f.arrow_data_type }}</span>
                </div>
              </div>
              <div class="schema-card">
                <div class="card-title">{{ ja.right_dataset.dataset_id }} ({{ ja.right_dataset.row_count | number }} rows)</div>
                <div *ngFor="let f of ja.right_dataset.fields" class="field-row" [class.join-key]="f.is_join_key">
                  <span>{{ f.field_name }}</span>
                  <span class="type-pill">{{ f.arrow_data_type }}</span>
                </div>
              </div>
            </div>

            <div *ngIf="!ja.type_compatibility.compatible" class="warning-box" data-testid="type-cast-warning">
              <strong>Type Cast Required:</strong> {{ ja.type_compatibility.warning }}
            </div>

            <div class="overlap-box" data-testid="overlap-venn-strip">
              <div class="overlap-labels">
                <span>Left Orphans: {{ ja.overlap_strip.left_orphan_count }}</span>
                <span>Matched: {{ ja.overlap_strip.matched_distinct_count }} (Jaccard {{ (ja.overlap_strip.jaccard_overlap * 100) | number:'1.0-0' }}%)</span>
                <span>Right Orphans: {{ ja.overlap_strip.right_orphan_count }}</span>
              </div>
              <div class="venn-bar">
                <div class="seg left-orphan" [style.flex]="ja.overlap_strip.left_orphan_count || 1"></div>
                <div class="seg matched" [style.flex]="ja.overlap_strip.matched_distinct_count || 1"></div>
                <div class="seg right-orphan" [style.flex]="ja.overlap_strip.right_orphan_count || 1"></div>
              </div>
            </div>

            <div *ngIf="ja.fan_out_diagnostics.is_fan_out_risk" class="warning-box" data-testid="fanout-diagnostics">
              <strong>Fan-Out Risk ({{ ja.fan_out_diagnostics.row_multiplier_estimate | number:'1.1-2' }}x):</strong>
              {{ ja.fan_out_diagnostics.explanation }}
              <pre *ngIf="ja.fan_out_diagnostics.safe_cte_sql_hint">{{ ja.fan_out_diagnostics.safe_cte_sql_hint }}</pre>
            </div>

            <div class="steward-actions">
              <button type="button" class="btn btn-verify" data-testid="btn-approve-join" (click)="approveActiveJoin()">
                ✓ Approve &amp; Verify
              </button>
              <button type="button" class="btn btn-reject" data-testid="btn-reject-join" (click)="rejectActiveJoin()">
                ✕ Reject as FORBIDDEN_JOIN
              </button>
              <button type="button" class="btn btn-explore" data-testid="btn-explore-joined-data" (click)="exploreJoinedData()">
                📊 Explore Joined Data
              </button>
            </div>
          </ng-container>

          <ng-container *ngIf="state.active_dataset_anatomy as da">
            <div class="anatomy-head">
              <h3>Dataset Schema Anatomy: {{ da.canonical_id }}</h3>
              <span class="de-badge de-badge-tested">{{ da.format }} • {{ da.row_count_estimate | number }} rows</span>
            </div>
            <div class="schema-tree" data-testid="dataset-schema-tree">
              <div *ngFor="let node of da.schema_tree" class="tree-node">
                <strong>{{ node.name }}</strong>
                <span class="type-pill">{{ node.arrow_data_type || node.node_kind }}</span>
                <div *ngFor="let child of node.children" class="tree-child">
                  ↳ {{ child.name }} <span class="type-pill">{{ child.arrow_data_type || child.node_kind }}</span>
                </div>
              </div>
            </div>
          </ng-container>
        </aside>
      </div>
    </div>
  `,
  styles: [`
    .topo-shell { display: flex; flex-direction: column; gap: 10px; padding: 14px; background: #0f172a; color: #f8fafc; height: 100%; overflow: auto; }
    .topo-header { display: flex; justify-content: space-between; align-items: center; gap: 10px; flex-wrap: wrap; border-bottom: 1px solid #1e293b; padding-bottom: 10px; }
    .lens-tabs { display: flex; gap: 6px; flex-wrap: wrap; }
    .tab { background: #1e293b; color: #cbd5e1; border: 1px solid #334155; border-radius: 6px; padding: 6px 12px; font-size: 12px; cursor: pointer; }
    .tab.active { background: #2563eb; border-color: #3b82f6; color: #fff; font-weight: 600; }
    .filter-chips { display: flex; gap: 6px; flex-wrap: wrap; }
    .degraded-banner { padding: 8px 12px; border-radius: 8px; background: rgba(245, 158, 11, 0.14); border: 1px solid rgba(245, 158, 11, 0.45); color: #fde68a; font-size: 12px; }
    .topo-body { display: grid; grid-template-columns: 1fr; gap: 12px; flex: 1; min-height: 0; }
    .topo-body.anatomy-mode { grid-template-columns: 1.1fr 1fr; }
    .graph-pane { display: flex; flex-direction: column; gap: 10px; }
    .cy-host { width: 100%; min-height: 260px; background: #111827; border: 1px solid #1f2937; border-radius: 10px; }
    .interactive-index { background: #111827; border: 1px solid #1f2937; border-radius: 10px; padding: 10px; }
    .interactive-index h4 { margin: 0 0 8px 0; font-size: 12px; color: #94a3b8; }
    .edge-row { width: 100%; display: flex; justify-content: space-between; align-items: center; gap: 8px; padding: 8px; margin-bottom: 6px; background: #1e293b; color: #e2e8f0; border: 1px solid #334155; border-radius: 6px; cursor: pointer; font-size: 12px; text-align: left; }
    .edge-row.is-forbidden { border-color: #ef4444; border-style: dashed; }
    .edge-row.is-confounder { border-color: #f59e0b; border-style: dashed; }
    .edge-meta { display: flex; gap: 6px; align-items: center; }
    .anatomy-pane { background: #111827; border: 1px solid #1f2937; border-radius: 10px; padding: 12px; display: flex; flex-direction: column; gap: 10px; }
    .anatomy-head { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
    .anatomy-head h3 { margin: 0; font-size: 14px; }
    .schema-pair { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
    .schema-card { background: #1e293b; border-radius: 8px; padding: 8px; font-size: 12px; }
    .card-title { font-weight: 600; margin-bottom: 6px; color: #93c5fd; }
    .field-row { display: flex; justify-content: space-between; padding: 4px 6px; border-radius: 4px; }
    .field-row.join-key { background: rgba(14, 165, 233, 0.22); border: 1px solid #38bdf8; font-weight: 600; }
    .type-pill { font-size: 10px; color: #94a3b8; }
    .overlap-box { background: #1e293b; border-radius: 8px; padding: 8px; font-size: 11px; }
    .overlap-labels { display: flex; justify-content: space-between; margin-bottom: 6px; color: #cbd5e1; }
    .venn-bar { display: flex; height: 12px; border-radius: 6px; overflow: hidden; gap: 2px; }
    .seg.left-orphan { background: #f59e0b; }
    .seg.matched { background: #10b981; }
    .seg.right-orphan { background: #64748b; }
    .warning-box { background: rgba(249, 115, 22, 0.14); border: 1px solid rgba(249, 115, 22, 0.45); border-radius: 8px; padding: 8px; font-size: 12px; color: #fed7aa; }
    .warning-box pre { margin: 6px 0 0; font-size: 11px; white-space: pre-wrap; color: #f8fafc; }
    .steward-actions { display: flex; flex-wrap: wrap; gap: 8px; }
    .btn { padding: 6px 10px; border-radius: 6px; border: 1px solid #334155; font-size: 12px; cursor: pointer; color: #fff; }
    .btn-verify { background: #059669; border-color: #10b981; }
    .btn-reject { background: #dc2626; border-color: #ef4444; }
    .btn-explore { background: #2563eb; border-color: #3b82f6; }
    .tree-node { padding: 4px 0; font-size: 12px; }
    .tree-child { padding-left: 14px; color: #cbd5e1; }
  `],
})
export class SemanticTopologyComponent
  implements AfterViewInit, OnDestroy, CanvasManagedComponent<SemanticTopologyState>
{
  private readonly api = inject(DataEngineApiService, { optional: true });

  @ViewChild('cyHost') cyHost?: ElementRef<HTMLDivElement>;
  @Input() state: SemanticTopologyState = defaultSemanticTopologyState();

  canvasId = 'semantic-topology';
  selectedLabel: string | null = null;
  aiAttributions: AiFieldAttribution[] = [];

  private mountProps: MfeMountProps<SemanticTopologyState> = {};
  private cy: CytoscapeCore | null = null;
  private viewInitialized = false;

  get visibleNodes(): TopologyNode[] {
    const allowed = new Set(this.state.visible_layers);
    return this.state.graph.nodes.filter((n) => allowed.has(n.layer));
  }

  get visibleEdges(): TopologyEdge[] {
    const ids = new Set(this.visibleNodes.map((n) => n.id));
    return this.state.graph.edges.filter((e) => ids.has(e.source_id) && ids.has(e.target_id));
  }

  ngAfterViewInit(): void {
    this.viewInitialized = true;
    this.renderGraph();
  }

  ngOnDestroy(): void {
    if (this.cy) {
      this.cy.destroy();
      this.cy = null;
    }
  }

  initializeFromProps(props: MfeMountProps<SemanticTopologyState>): void {
    this.mountProps = props;
    this.canvasId = props.id ?? this.canvasId;
    const incoming = props.initialState ?? props.content;
    if (incoming) {
      this.state = {
        ...defaultSemanticTopologyState(),
        ...incoming,
      };
    }
    if (this.viewInitialized) {
      this.renderGraph();
    }
  }

  applyPatch(
    patches: JsonPatchOp[] | Partial<SemanticTopologyState>,
    attributions?: AiFieldAttribution[],
  ): void {
    this.state = applyJsonPatches(this.state, patches);
    if (attributions && attributions.length > 0) {
      this.aiAttributions = [...this.aiAttributions, ...attributions];
    }
    if (this.viewInitialized) {
      this.syncGraphInPlace();
    }
  }

  getState(): SemanticTopologyState {
    return JSON.parse(JSON.stringify(this.state));
  }

  getSchema(): Record<string, any> {
    return {
      $schema: 'http://json-schema.org/draft-07/schema#',
      title: 'SemanticTopologyState',
      type: 'object',
      properties: {
        lens: {
          type: 'string',
          enum: ['semantic_3layer', 'er_join', 'field_anatomy', 'lineage_overlay'],
        },
        visible_layers: { type: 'array', items: { type: 'integer', enum: [0, 1, 2] } },
        epistemic_filters: { type: 'array', items: { type: 'string' } },
        focus_node_id: { type: ['string', 'null'] },
        expanded_datasets: { type: 'array', items: { type: 'string' } },
        highlighted_path: { type: ['object', 'null'] },
      },
      'x-actions': [
        'verify_join',
        'reject_forbidden_join',
        'probe_join',
        'inspect_dataset_form',
        'explore_data_visual',
        'open_provenance',
      ],
    };
  }

  setLens(lens: TopologyLens): void {
    this.state = { ...this.state, lens };
    this.mountProps.onStateChange?.(this.getState(), [{ op: 'replace', path: '/lens', value: lens }]);
    this.syncGraphInPlace();
  }

  selectNode(node: TopologyNode): void {
    this.selectedLabel = `${node.label} (${node.epistemic_status})`;
    this.mountProps.onSelectionChange?.({
      canvasId: this.canvasId,
      path: `/nodes/${node.id}`,
      id: node.id,
      label: this.selectedLabel,
      context: { node },
    });
  }

  selectEdge(edge: TopologyEdge): void {
    const label = `${edge.source_id} -> ${edge.target_id} (${edge.cardinality ?? edge.predicate})`;
    this.selectedLabel = label;
    this.mountProps.onSelectionChange?.({
      canvasId: this.canvasId,
      path: `/edges/${edge.id}`,
      id: edge.id,
      label,
      context: { edge },
    });
  }

  async approveActiveJoin(): Promise<void> {
    const ja = this.state.active_join_anatomy;
    if (!ja) return;
    const payload = {
      epistemic_status: 'verified',
      verified_by: 'steward_user',
      steward_notes: 'Verified via SemanticTopology Field Anatomy',
    };
    ja.join.epistemic_status = 'verified';
    this.mountProps.onAction?.('verify_join', { join_id: ja.join.id, ...payload });
    if (this.api) {
      try {
        await this.api.stewardJoin(ja.join.id, payload);
      } catch {
        // Non-blocking in offline preview
      }
    }
  }

  async rejectActiveJoin(): Promise<void> {
    const ja = this.state.active_join_anatomy;
    if (!ja) return;
    const payload = {
      epistemic_status: 'rejected',
      verified_by: 'steward_user',
      steward_notes: 'Rejected as FORBIDDEN_JOIN via SemanticTopology',
    };
    ja.join.epistemic_status = 'rejected';
    ja.join.predicate = 'FORBIDDEN_JOIN';
    this.mountProps.onAction?.('reject_forbidden_join', { join_id: ja.join.id, ...payload });
    if (this.api) {
      try {
        await this.api.stewardJoin(ja.join.id, payload);
      } catch {
        // Non-blocking in offline preview
      }
    }
  }

  exploreJoinedData(): void {
    const ja = this.state.active_join_anatomy;
    if (!ja) return;
    this.mountProps.onAction?.('explore_data_visual', {
      join_id: ja.join.id,
      sql: ja.explore_sql_template,
      left_dataset: ja.left_dataset.dataset_id,
      right_dataset: ja.right_dataset.dataset_id,
    });
  }

  edgeCssClass(edge: TopologyEdge): string {
    if (edge.predicate === 'FORBIDDEN_JOIN') return 'is-forbidden';
    if (edge.predicate === 'CONFOUNDED_BY') return 'is-confounder';
    return '';
  }

  statusBadgeClass(status: string, predicate?: string): string {
    if (predicate === 'FORBIDDEN_JOIN' || status === 'rejected') return 'de-badge-forbidden';
    if (status === 'verified') return 'de-badge-verified';
    if (status === 'drifted') return 'de-badge-drifted';
    if (status === 'empirically_tested') return 'de-badge-tested';
    return 'de-badge-hypothesized';
  }

  private renderGraph(): void {
    const host = this.cyHost?.nativeElement;
    if (!host) return;
    const elements = buildCytoscapeElements(this.state);
    host.setAttribute('data-lens', this.state.lens);
    host.setAttribute('data-element-count', String(elements.length));

    const canMountCanvas = typeof HTMLCanvasElement !== 'undefined' && host.clientWidth > 0;
    if (!canMountCanvas) {
      return;
    }

    ensureElkRegistered();
    this.cy = cytoscape({
      container: host,
      elements,
      style: [
        {
          selector: 'node',
          style: {
            label: 'data(label)',
            color: '#f8fafc',
            'font-size': 11,
            'text-valign': 'center',
            'background-color': '#1e293b',
            'border-width': 2,
            'border-color': '#64748b',
            shape: 'round-rectangle',
            padding: '8px',
          },
        },
        { selector: 'node.status-verified', style: { 'border-color': '#10b981' } },
        { selector: 'node.status-empirically_tested', style: { 'border-color': '#0ea5e9' } },
        { selector: 'node.is-drifted', style: { 'border-color': '#f59e0b', 'border-width': 3 } },
        { selector: 'node.layer-2', style: { 'background-color': '#1e1b4b', 'border-color': '#818cf8' } },
        {
          selector: 'edge',
          style: {
            width: 2,
            label: 'data(label)',
            color: '#cbd5e1',
            'font-size': 10,
            'curve-style': 'bezier',
            'target-arrow-shape': 'triangle',
            'line-color': '#64748b',
            'target-arrow-color': '#64748b',
          },
        },
        { selector: 'edge.status-verified', style: { 'line-color': '#10b981', 'target-arrow-color': '#10b981' } },
        { selector: 'edge.status-empirically_tested', style: { 'line-color': '#0ea5e9', 'target-arrow-color': '#0ea5e9' } },
        { selector: 'edge.is-forbidden', style: { 'line-color': '#ef4444', 'target-arrow-color': '#ef4444', 'line-style': 'dashed' } },
        { selector: 'edge.is-confounder', style: { 'line-color': '#f59e0b', 'target-arrow-color': '#f59e0b', 'line-style': 'dashed' } },
        { selector: 'edge.has-fanout', style: { 'line-color': '#f97316', 'target-arrow-color': '#f97316', width: 3 } },
        { selector: '.is-dimmed', style: { opacity: 0.2 } },
        { selector: '.is-highlighted', style: { 'border-width': 4, width: 4 } },
      ],
      layout: { name: 'elk', elk: { algorithm: 'layered', 'elk.direction': 'DOWN' } } as any,
    });

    this.cy.on('tap', 'node', (evt) => {
      const id = evt.target.id();
      const found = this.state.graph.nodes.find((n) => n.id === id);
      if (found) this.selectNode(found);
    });
    this.cy.on('tap', 'edge', (evt) => {
      const id = evt.target.id();
      const found = this.state.graph.edges.find((e) => e.id === id);
      if (found) this.selectEdge(found);
    });
  }

  private syncGraphInPlace(): void {
    const host = this.cyHost?.nativeElement;
    const elements = buildCytoscapeElements(this.state);
    if (host) {
      host.setAttribute('data-lens', this.state.lens);
      host.setAttribute('data-element-count', String(elements.length));
    }
    if (!this.cy) return;
    const zoom = this.cy.zoom();
    const pan = this.cy.pan();
    this.cy.batch(() => {
      this.cy!.elements().remove();
      this.cy!.add(elements);
    });
    this.cy.zoom(zoom);
    this.cy.pan(pan);
  }
}
