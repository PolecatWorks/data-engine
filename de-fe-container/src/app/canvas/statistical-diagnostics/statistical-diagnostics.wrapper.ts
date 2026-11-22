import { Component, Input, inject } from '@angular/core';
import { CommonModule } from '@angular/common';
import {
  AiFieldAttribution,
  CanvasManagedComponent,
  JsonPatchOp,
  MfeMountProps,
  applyJsonPatches,
  createCanvasWrapper,
} from '../mfe-canvas.types';
import { DataEngineApiService } from '../../services/data-engine-api.service';

export interface StatisticalDiagnosticsState {
  tool: string;
  sample_context: {
    population_rows: number;
    analyzed_rows: number;
    sampled: boolean;
    sampling_seed?: number | null;
  };
  primary_result: Record<string, any>;
  assumption_checks: Array<{
    check: string;
    status: 'passed' | 'warning' | 'failed';
    statistic?: number | null;
    p_value?: number | null;
    message: string;
  }>;
  recommended_alternative?: {
    method: string;
    reason: string;
    result: Record<string, any>;
  } | null;
  diagnostic_plots: Array<{ plot_type: string; title: string; series: any }>;
  learned_field_traits: Array<{
    field_canonical_id: string;
    distribution_family?: string;
    is_heavy_tailed?: boolean;
    recommended_test_family?: string;
    epistemic_status: string;
  }>;
  provenance_run_id: string;
}

export function defaultStatisticalDiagnosticsState(): StatisticalDiagnosticsState {
  return {
    tool: 'stat_hypothesis_test',
    sample_context: { population_rows: 12450, analyzed_rows: 12450, sampled: false, sampling_seed: null },
    primary_result: { method: 'welch_t_test', p_value: 0.016, effect_size: { metric: 'hedges_g', value: 0.043 } },
    assumption_checks: [
      { check: 'normality_shapiro_or_dagostino', status: 'failed', p_value: 0.0001, message: 'Heavy right skew detected.' },
      { check: 'sample_size_adequacy', status: 'passed', message: 'Sufficient sample size per arm.' },
    ],
    recommended_alternative: {
      method: 'mann_whitney_u',
      reason: 'Non-normal heavy-tailed distribution; non-parametric fallback computed.',
      result: { p_value: 0.412 },
    },
    diagnostic_plots: [
      { plot_type: 'qq_plot', title: 'Q-Q Plot', series: [] },
      { plot_type: 'distribution_histogram', title: 'Group Distributions', series: [] },
    ],
    learned_field_traits: [
      {
        field_canonical_id: 'sales_orders.order_total_usd',
        distribution_family: 'log_normal_heavy_tailed',
        is_heavy_tailed: true,
        recommended_test_family: 'non_parametric_or_log_transform',
        epistemic_status: 'empirically_tested',
      },
    ],
    provenance_run_id: 'prov-stat-demo-01',
  };
}

@Component({
  selector: 'de-statistical-diagnostics',
  standalone: true,
  imports: [CommonModule],
  template: `
    <div class="stat-shell" data-testid="statistical-diagnostics">
      <header class="stat-head">
        <h3>Statistical Diagnostics — {{ state.tool }}</h3>
        <span class="de-badge de-badge-tested">Run: {{ state.provenance_run_id }}</span>
      </header>

      <div *ngIf="state.sample_context.sampled" class="sampling-banner" data-testid="stat-sampling-banner">
        Sampled {{ state.sample_context.analyzed_rows }} of {{ state.sample_context.population_rows }} rows
        (seed={{ state.sample_context.sampling_seed }})
      </div>

      <section class="checks-list" data-testid="assumption-scorecard">
        <div *ngFor="let c of state.assumption_checks" class="check-row">
          <span class="de-badge" [ngClass]="checkBadge(c.status)">{{ c.status }}</span>
          <strong>{{ c.check }}</strong>
          <span>{{ c.message }}</span>
        </div>
      </section>

      <section *ngIf="state.recommended_alternative as alt" class="alt-banner" data-testid="recommended-alternative">
        <strong>Recommended Alternative ({{ alt.method }}):</strong> {{ alt.reason }}
      </section>

      <section class="traits-list" data-testid="learned-traits">
        <div *ngFor="let t of state.learned_field_traits" class="trait-row">
          <span><strong>{{ t.field_canonical_id }}</strong> ({{ t.distribution_family }}) — {{ t.epistemic_status }}</span>
          <button type="button" class="btn-verify" data-testid="btn-verify-trait" (click)="verifyTrait(t)">
            ✓ Verify Statistical Trait
          </button>
        </div>
      </section>
    </div>
  `,
  styles: [`
    .stat-shell { display: flex; flex-direction: column; gap: 12px; padding: 16px; background: #0f172a; color: #f8fafc; height: 100%; overflow: auto; }
    .stat-head { display: flex; justify-content: space-between; align-items: center; }
    .stat-head h3 { margin: 0; font-size: 15px; }
    .sampling-banner, .alt-banner { padding: 8px 12px; border-radius: 8px; background: rgba(245, 158, 11, 0.14); border: 1px solid rgba(245, 158, 11, 0.45); font-size: 12px; color: #fde68a; }
    .checks-list, .traits-list { display: flex; flex-direction: column; gap: 8px; }
    .check-row, .trait-row { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 8px; background: #1e293b; border-radius: 8px; font-size: 12px; }
    .btn-verify { background: #059669; color: #fff; border: 1px solid #10b981; border-radius: 6px; padding: 5px 10px; font-size: 12px; cursor: pointer; }
  `],
})
export class StatisticalDiagnosticsComponent implements CanvasManagedComponent<StatisticalDiagnosticsState> {
  private readonly api = inject(DataEngineApiService, { optional: true });
  @Input() state: StatisticalDiagnosticsState = defaultStatisticalDiagnosticsState();
  private mountProps: MfeMountProps<StatisticalDiagnosticsState> = {};

  initializeFromProps(props: MfeMountProps<StatisticalDiagnosticsState>): void {
    this.mountProps = props;
    const incoming = props.initialState ?? props.content;
    if (incoming) {
      this.state = { ...defaultStatisticalDiagnosticsState(), ...incoming };
    }
  }

  applyPatch(patches: JsonPatchOp[] | Partial<StatisticalDiagnosticsState>, _attr?: AiFieldAttribution[]): void {
    this.state = applyJsonPatches(this.state, patches);
  }

  getState(): StatisticalDiagnosticsState {
    return JSON.parse(JSON.stringify(this.state));
  }

  getSchema(): Record<string, any> {
    return {
      $schema: 'http://json-schema.org/draft-07/schema#',
      title: 'StatisticalDiagnosticsState',
      type: 'object',
      properties: {
        tool: { type: 'string' },
        provenance_run_id: { type: 'string' },
      },
      'x-actions': ['verify_statistical_trait', 'open_provenance'],
    };
  }

  checkBadge(status: string): string {
    if (status === 'passed') return 'de-badge-verified';
    if (status === 'warning') return 'de-badge-drifted';
    return 'de-badge-forbidden';
  }

  async verifyTrait(trait: StatisticalDiagnosticsState['learned_field_traits'][number]): Promise<void> {
    trait.epistemic_status = 'verified';
    this.mountProps.onAction?.('verify_statistical_trait', { field_canonical_id: trait.field_canonical_id });
    if (this.api) {
      try {
        await this.api.verifyStatisticalField(trait.field_canonical_id, {
          verified_by: 'steward_user',
          recommended_test_family: trait.recommended_test_family,
        });
      } catch {
        // Non-blocking in offline preview
      }
    }
  }
}

const wrapper = createCanvasWrapper<StatisticalDiagnosticsState, StatisticalDiagnosticsComponent>(
  StatisticalDiagnosticsComponent,
);
export const mount = wrapper.mount;
export const unmount = wrapper.unmount;
