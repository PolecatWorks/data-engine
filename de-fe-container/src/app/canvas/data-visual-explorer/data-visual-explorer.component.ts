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
import embed, { Result as VegaEmbedResult } from 'vega-embed';
import * as echarts from 'echarts';

import {
  AiFieldAttribution,
  CanvasManagedComponent,
  JsonPatchOp,
  MfeMountProps,
  applyJsonPatches,
} from '../mfe-canvas.types';
import {
  ChartRecommendation,
  DataEngineApiService,
  DataVisualSpec,
  ProfileOverlays,
  VisualProvenance,
} from '../../services/data-engine-api.service';

export interface DataVisualExplorerState {
  visual_id: string;
  title: string;
  renderer: 'vega_lite' | 'echarts';
  chart_family: string;
  data_binding: Record<string, any>;
  spec: Record<string, any>;
  data_values: Record<string, any>[];
  active_filters: Record<string, any>;
  profile_overlays: ProfileOverlays;
  provenance?: VisualProvenance | null;
  recommendations: ChartRecommendation[];
}

export function defaultDataVisualExplorerState(): DataVisualExplorerState {
  return {
    visual_id: 'demo-daily-revenue',
    title: 'Daily Net Revenue by Customer Segment',
    renderer: 'vega_lite',
    chart_family: 'bar',
    data_binding: { kind: 'dataset', dataset_id: 'sales_orders' },
    spec: {
      $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
      mark: { type: 'bar', cornerRadiusTopLeft: 4, cornerRadiusTopRight: 4 },
      encoding: {
        x: { field: 'segment', type: 'nominal', title: 'Segment' },
        y: { field: 'net_revenue', type: 'quantitative', scale: { type: 'linear' }, title: 'Net Revenue (USD)' },
        color: { field: 'segment', type: 'nominal' },
      },
    },
    data_values: [
      { segment: 'Enterprise', net_revenue: 142500, orders: 420 },
      { segment: 'Mid-Market', net_revenue: 86400, orders: 510 },
      { segment: 'SMB', net_revenue: 54200, orders: 780 },
    ],
    active_filters: {},
    profile_overlays: {
      reference_lines: [{ axis: 'y', value: 94366, label: 'Mean Segment Revenue', style: 'dashed' }],
      annotations: [],
      log_scale_recommended: false,
      seasonality_period: null,
    },
    provenance: {
      provenance_run_id: 'prov-demo-visual-001',
      trust_summary: {
        all_rules_human_verified: true,
        min_rule_confidence: 0.97,
        verified_by_stewards: ['steward_alice'],
        pre_aggregation_applied: true,
        unit_conversion_applied: false,
      },
      sampled: false,
      population_rows: 1710,
      rendered_rows: 3,
      sampling_seed: null,
    },
    recommendations: [],
  };
}

@Component({
  selector: 'de-data-visual-explorer',
  standalone: true,
  imports: [CommonModule, FormsModule],
  template: `
    <div class="explorer-shell" data-testid="data-visual-explorer">
      <header class="explorer-header">
        <div>
          <h2 class="title">{{ state.title }}</h2>
          <div class="meta-row">
            <span class="de-badge" [ngClass]="isVerified ? 'de-badge-verified' : 'de-badge-drifted'" data-testid="trust-badge">
              {{ isVerified ? '✓ Verified Provenance' : '⚠ AI-Inferred Rules' }}
            </span>
            <span class="de-badge de-badge-tested">{{ state.renderer === 'vega_lite' ? 'Vega-Lite v5' : 'Apache ECharts' }}</span>
            <span class="de-badge de-badge-hypothesized">{{ state.chart_family }}</span>
            <span *ngIf="aiAttributions.length > 0" class="de-badge de-badge-tested" data-testid="ai-attribution-badge">
              ✨ AI Patched ({{ aiAttributions.length }})
            </span>
          </div>
        </div>
        <div class="toolbar-actions">
          <button type="button" class="btn" data-testid="btn-toggle-renderer" (click)="toggleRenderer()">
            Switch to {{ state.renderer === 'vega_lite' ? 'ECharts' : 'Vega-Lite' }}
          </button>
          <button type="button" class="btn" data-testid="btn-toggle-log" (click)="toggleLogScale()">
            Scale: {{ currentYScale }}
          </button>
          <button type="button" class="btn btn-accent" data-testid="btn-run-stat-check" (click)="triggerStatisticalCheck()">
            Run Statistical Check
          </button>
          <button type="button" class="btn" data-testid="btn-open-provenance" (click)="triggerOpenProvenance()">
            Lineage
          </button>
          <button type="button" class="btn" data-testid="btn-save-visual" (click)="triggerSaveVisual()">
            Save
          </button>
        </div>
      </header>

      <div *ngIf="state.provenance?.sampled" class="sampling-banner" data-testid="sampling-disclosure-banner">
        <strong>Sampling Disclosure:</strong>
        Sampled {{ state.provenance?.rendered_rows | number }} of {{ state.provenance?.population_rows | number }} rows
        (seed={{ state.provenance?.sampling_seed }}).
      </div>

      <section *ngIf="state.recommendations.length > 0" class="rec-strip" data-testid="recommendation-strip">
        <span class="rec-label">Recommended:</span>
        <button
          *ngFor="let rec of state.recommendations"
          type="button"
          class="rec-chip"
          [title]="rec.rationale"
          (click)="applyRecommendation(rec)">
          #{{ rec.rank }} {{ rec.visual_spec.chart_family }} ({{ (rec.score * 100) | number:'1.0-0' }}%)
        </button>
      </section>

      <section class="encoding-shelf" data-testid="encoding-shelf">
        <label>
          <span>Chart</span>
          <select [ngModel]="state.chart_family" (ngModelChange)="onChartFamilyChange($event)">
            <option value="bar">Bar</option>
            <option value="line">Line</option>
            <option value="area">Area</option>
            <option value="scatter">Scatter</option>
            <option value="box_plot">Box Plot</option>
            <option value="heatmap">Heatmap</option>
            <option value="histogram">Histogram</option>
          </select>
        </label>
        <label>
          <span>X Field</span>
          <input [ngModel]="xField" (ngModelChange)="onEncodingFieldChange('x', $event)" />
        </label>
        <label>
          <span>Y Field</span>
          <input [ngModel]="yField" (ngModelChange)="onEncodingFieldChange('y', $event)" />
        </label>
        <label>
          <span>Color</span>
          <input [ngModel]="colorField" (ngModelChange)="onEncodingFieldChange('color', $event)" />
        </label>
      </section>

      <div class="chart-stage" data-testid="chart-stage">
        <div #chartHost class="chart-host" data-testid="chart-host"></div>
        <div *ngIf="state.profile_overlays.annotations.length > 0" class="annotations-bar" data-testid="annotations-bar">
          <span *ngFor="let ann of state.profile_overlays.annotations" class="de-badge de-badge-warning">
            📌 {{ ann.label }} ({{ ann.x }})
          </span>
        </div>
      </div>

      <section class="data-drawer" data-testid="data-drawer">
        <div class="drawer-head">
          <span>Rendered Rows ({{ state.data_values.length }}) — Click a row to focus in chat</span>
          <span *ngIf="selectedSummary" class="de-badge de-badge-tested" data-testid="active-selection-chip">
            🎯 {{ selectedSummary }}
          </span>
        </div>
        <div class="table-scroll" *ngIf="state.data_values.length > 0">
          <table>
            <thead>
              <tr>
                <th *ngFor="let col of tableColumns">{{ col }}</th>
              </tr>
            </thead>
            <tbody>
              <tr *ngFor="let row of state.data_values; let i = index" (click)="selectDatum(row, i)">
                <td *ngFor="let col of tableColumns">{{ row[col] }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
    </div>
  `,
  styles: [`
    .explorer-shell { display: flex; flex-direction: column; gap: 12px; padding: 16px; background: #0f172a; color: #f8fafc; height: 100%; overflow: auto; }
    .explorer-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 12px; flex-wrap: wrap; border-bottom: 1px solid #1e293b; padding-bottom: 12px; }
    .title { margin: 0 0 6px 0; font-size: 16px; font-weight: 600; }
    .meta-row { display: flex; flex-wrap: wrap; gap: 6px; }
    .toolbar-actions { display: flex; flex-wrap: wrap; gap: 6px; }
    .btn { background: #1e293b; color: #e2e8f0; border: 1px solid #334155; border-radius: 6px; padding: 6px 10px; font-size: 12px; cursor: pointer; }
    .btn:hover { background: #334155; }
    .btn-accent { background: #2563eb; border-color: #3b82f6; color: #fff; }
    .sampling-banner { padding: 8px 12px; border-radius: 8px; background: rgba(245, 158, 11, 0.14); border: 1px solid rgba(245, 158, 11, 0.45); color: #fde68a; font-size: 12px; }
    .rec-strip { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
    .rec-label { font-size: 12px; color: #94a3b8; }
    .rec-chip { background: #1e293b; color: #93c5fd; border: 1px solid #334155; border-radius: 999px; padding: 4px 10px; font-size: 11px; cursor: pointer; }
    .encoding-shelf { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 8px; background: #1e293b; padding: 10px; border-radius: 8px; }
    .encoding-shelf label { display: flex; flex-direction: column; gap: 4px; font-size: 11px; color: #94a3b8; }
    .encoding-shelf input, .encoding-shelf select { background: #0f172a; color: #f8fafc; border: 1px solid #334155; border-radius: 4px; padding: 4px 6px; font-size: 12px; }
    .chart-stage { background: #111827; border: 1px solid #1f2937; border-radius: 10px; padding: 12px; min-height: 260px; display: flex; flex-direction: column; gap: 8px; }
    .chart-host { width: 100%; min-height: 240px; }
    .annotations-bar { display: flex; flex-wrap: wrap; gap: 6px; }
    .data-drawer { background: #111827; border: 1px solid #1f2937; border-radius: 10px; padding: 10px; }
    .drawer-head { display: flex; justify-content: space-between; align-items: center; font-size: 12px; color: #94a3b8; margin-bottom: 8px; }
    .table-scroll { max-height: 180px; overflow: auto; }
    table { width: 100%; border-collapse: collapse; font-size: 12px; }
    th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid #1e293b; }
    tbody tr { cursor: pointer; }
    tbody tr:hover { background: #1e293b; }
  `],
})
export class DataVisualExplorerComponent
  implements AfterViewInit, OnDestroy, CanvasManagedComponent<DataVisualExplorerState>
{
  private readonly api = inject(DataEngineApiService, { optional: true });

  @ViewChild('chartHost') chartHost?: ElementRef<HTMLDivElement>;
  @Input() state: DataVisualExplorerState = defaultDataVisualExplorerState();

  canvasId = 'data-visual-explorer';
  aiAttributions: AiFieldAttribution[] = [];
  selectedSummary: string | null = null;
  renderCount = 0;

  private mountProps: MfeMountProps<DataVisualExplorerState> = {};
  private vegaResult: VegaEmbedResult | null = null;
  private echartsInstance: echarts.ECharts | null = null;
  private viewInitialized = false;

  get isVerified(): boolean {
    return this.state.provenance?.trust_summary?.all_rules_human_verified ?? false;
  }

  get currentYScale(): string {
    return this.state.spec?.['encoding']?.['y']?.['scale']?.['type'] ?? 'linear';
  }

  get xField(): string {
    return this.state.spec?.['encoding']?.['x']?.['field'] ?? '';
  }

  get yField(): string {
    return this.state.spec?.['encoding']?.['y']?.['field'] ?? '';
  }

  get colorField(): string {
    return this.state.spec?.['encoding']?.['color']?.['field'] ?? '';
  }

  get tableColumns(): string[] {
    const first = this.state.data_values[0];
    return first ? Object.keys(first) : [];
  }

  ngAfterViewInit(): void {
    this.viewInitialized = true;
    void this.renderVisual();
  }

  ngOnDestroy(): void {
    this.cleanupRenderers();
  }

  initializeFromProps(props: MfeMountProps<DataVisualExplorerState>): void {
    this.mountProps = props;
    this.canvasId = props.id ?? this.canvasId;
    const incoming = props.initialState ?? props.content;
    if (incoming) {
      this.state = {
        ...defaultDataVisualExplorerState(),
        ...incoming,
        profile_overlays: {
          ...(incoming.profile_overlays ?? {}),
          reference_lines: incoming.profile_overlays?.reference_lines ?? [],
          annotations: incoming.profile_overlays?.annotations ?? [],
        },
      };
    }
    if (this.viewInitialized) {
      void this.renderVisual();
    }
  }

  applyPatch(
    patches: JsonPatchOp[] | Partial<DataVisualExplorerState>,
    attributions?: AiFieldAttribution[],
  ): void {
    this.state = applyJsonPatches(this.state, patches);
    if (attributions && attributions.length > 0) {
      this.aiAttributions = [...this.aiAttributions, ...attributions];
    }
    if (this.viewInitialized) {
      void this.renderVisual();
    }
  }

  getState(): DataVisualExplorerState {
    return JSON.parse(JSON.stringify(this.state));
  }

  getSchema(): Record<string, any> {
    return {
      $schema: 'http://json-schema.org/draft-07/schema#',
      title: 'DataVisualExplorerState',
      type: 'object',
      properties: {
        visual_id: { type: 'string' },
        title: { type: 'string' },
        renderer: { type: 'string', enum: ['vega_lite', 'echarts'] },
        chart_family: {
          type: 'string',
          enum: ['bar', 'grouped_bar', 'stacked_bar', 'line', 'area', 'scatter', 'bubble', 'histogram', 'box_plot', 'violin', 'heatmap', 'pivot_table', 'kpi_sparkline', 'cohort_matrix', 'sankey'],
        },
        spec: { type: 'object' },
        active_filters: { type: 'object' },
        profile_overlays: {
          type: 'object',
          properties: {
            reference_lines: { type: 'array' },
            annotations: { type: 'array' },
            log_scale_recommended: { type: 'boolean' },
          },
        },
      },
      'x-actions': [
        'run_statistical_check',
        'open_provenance',
        'save_visual',
        'drill_down_rows',
        'export_spec',
      ],
    };
  }

  toggleRenderer(): void {
    const next: 'vega_lite' | 'echarts' = this.state.renderer === 'vega_lite' ? 'echarts' : 'vega_lite';
    this.state = { ...this.state, renderer: next };
    this.emitStateChange([{ op: 'replace', path: '/renderer', value: next }]);
    void this.renderVisual();
  }

  toggleLogScale(): void {
    const nextScale = this.currentYScale === 'log' ? 'linear' : 'log';
    this.state = applyJsonPatches(this.state, [
      { op: 'replace', path: '/spec/encoding/y/scale/type', value: nextScale },
    ]);
    this.emitStateChange([{ op: 'replace', path: '/spec/encoding/y/scale/type', value: nextScale }]);
    void this.renderVisual();
  }

  onChartFamilyChange(family: string): void {
    const markMap: Record<string, string> = {
      bar: 'bar',
      line: 'line',
      area: 'area',
      scatter: 'point',
      box_plot: 'boxplot',
      heatmap: 'rect',
      histogram: 'bar',
    };
    const mark = markMap[family] ?? 'bar';
    this.state = applyJsonPatches(this.state, [
      { op: 'replace', path: '/chart_family', value: family },
      { op: 'replace', path: '/spec/mark', value: mark },
    ]);
    this.emitStateChange([
      { op: 'replace', path: '/chart_family', value: family },
      { op: 'replace', path: '/spec/mark', value: mark },
    ]);
    void this.renderVisual();
  }

  onEncodingFieldChange(channel: 'x' | 'y' | 'color', field: string): void {
    this.state = applyJsonPatches(this.state, [
      { op: 'replace', path: `/spec/encoding/${channel}/field`, value: field },
    ]);
    this.emitStateChange([
      { op: 'replace', path: `/spec/encoding/${channel}/field`, value: field },
    ]);
    void this.renderVisual();
  }

  applyRecommendation(rec: ChartRecommendation): void {
    const vs: DataVisualSpec = rec.visual_spec;
    this.state = {
      ...this.state,
      visual_id: vs.visual_id,
      title: vs.title,
      renderer: vs.renderer,
      chart_family: vs.chart_family,
      data_binding: vs.data_binding,
      spec: vs.spec,
      profile_overlays: vs.profile_overlays ?? { reference_lines: [], annotations: [] },
    };
    this.emitStateChange([{ op: 'replace', path: '/chart_family', value: vs.chart_family }]);
    void this.renderVisual();
  }

  selectDatum(row: Record<string, any>, index: number): void {
    const summary = Object.entries(row)
      .slice(0, 2)
      .map(([k, v]) => `${k}=${v}`)
      .join(', ');
    this.selectedSummary = summary;
    this.mountProps.onSelectionChange?.({
      canvasId: this.canvasId,
      path: `/selection/datum/${index}`,
      id: String(index),
      label: summary,
      context: { datum: row },
    });
  }

  triggerStatisticalCheck(): void {
    const cols = [this.xField, this.yField, this.colorField].filter(Boolean);
    this.mountProps.onAction?.('run_statistical_check', {
      data_binding: this.state.data_binding,
      columns: cols,
    });
  }

  triggerOpenProvenance(): void {
    this.mountProps.onAction?.('open_provenance', {
      provenance_run_id: this.state.provenance?.provenance_run_id ?? null,
    });
  }

  async triggerSaveVisual(): Promise<void> {
    const payload = {
      name: this.state.title,
      description: 'Saved from DataVisualExplorer',
      renderer: this.state.renderer,
      chart_family: this.state.chart_family,
      primary_dataset_id: this.state.data_binding?.['dataset_id'] ?? null,
      data_binding: this.state.data_binding,
      visual_spec: this.state.spec,
      profile_overlays: this.state.profile_overlays,
      last_provenance_run_id: this.state.provenance?.provenance_run_id ?? null,
    };
    this.mountProps.onAction?.('save_visual', payload);
    if (this.api) {
      try {
        await this.api.saveVisual(payload);
      } catch {
        // Non-blocking when previewed without live backend
      }
    }
  }

  private emitStateChange(patches: JsonPatchOp[]): void {
    this.mountProps.onStateChange?.(this.getState(), patches);
  }

  private cleanupRenderers(): void {
    if (this.vegaResult) {
      this.vegaResult.finalize();
      this.vegaResult = null;
    }
    if (this.echartsInstance) {
      this.echartsInstance.dispose();
      this.echartsInstance = null;
    }
  }

  private async renderVisual(): Promise<void> {
    const host = this.chartHost?.nativeElement;
    if (!host) return;

    this.renderCount += 1;
    this.cleanupRenderers();

    // Guard for headless jsdom unit-test environments lacking Canvas 2D context
    const hasCanvas = typeof HTMLCanvasElement !== 'undefined' && host.clientWidth > 0;
    if (!hasCanvas) {
      host.setAttribute('data-rendered-mode', this.state.renderer);
      host.setAttribute('data-chart-family', this.state.chart_family);
      host.setAttribute('data-y-scale', this.currentYScale);
      return;
    }

    if (this.state.renderer === 'echarts') {
      this.echartsInstance = echarts.init(host, 'dark');
      const xKey = this.xField || this.tableColumns[0] || 'x';
      const yKey = this.yField || this.tableColumns[1] || 'y';
      const echartsOption: any = this.state.spec?.['series']
        ? this.state.spec
        : {
            tooltip: { trigger: 'axis' },
            xAxis: { type: 'category', data: this.state.data_values.map((d) => d[xKey]) },
            yAxis: { type: this.currentYScale === 'log' ? 'log' : 'value' },
            series: [
              {
                name: yKey,
                type: this.state.chart_family === 'line' ? 'line' : 'bar',
                data: this.state.data_values.map((d) => d[yKey]),
              },
            ],
          };
      this.echartsInstance.setOption(echartsOption);
      this.echartsInstance.on('click', (params: any) => {
        const idx = typeof params.dataIndex === 'number' ? params.dataIndex : 0;
        const row = this.state.data_values[idx] ?? { name: params.name, value: params.value };
        this.selectDatum(row, idx);
      });
    } else {
      const vlSpec: any = {
        width: 'container',
        height: 240,
        background: 'transparent',
        config: {
          axis: { labelColor: '#cbd5e1', titleColor: '#e2e8f0', gridColor: '#1e293b' },
          legend: { labelColor: '#cbd5e1', titleColor: '#e2e8f0' },
        },
        ...this.state.spec,
        data: { values: this.state.data_values },
      };
      try {
        this.vegaResult = await embed(host, vlSpec, { actions: false, renderer: 'svg' });
        this.vegaResult.view.addEventListener('click', (_evt, item) => {
          if (item?.datum) {
            this.selectDatum(item.datum as Record<string, any>, 0);
          }
        });
      } catch {
        host.setAttribute('data-rendered-mode', 'vega_lite_fallback');
      }
    }
  }
}
