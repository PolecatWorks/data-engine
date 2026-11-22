import { Component, Input } from '@angular/core';
import { CommonModule } from '@angular/common';
import {
  AiFieldAttribution,
  CanvasManagedComponent,
  JsonPatchOp,
  MfeMountProps,
  applyJsonPatches,
  createCanvasWrapper,
} from '../mfe-canvas.types';

export interface JoinHypothesisReviewState {
  join_id: string;
  left_field: string;
  right_field: string;
  cardinality: string;
  inclusion_ratio: number;
  epistemic_status: string;
  steward_notes?: string;
}

@Component({
  selector: 'de-join-hypothesis-review',
  standalone: true,
  imports: [CommonModule],
  template: `
    <div class="panel" data-testid="join-hypothesis-review">
      <h3>Join Hypothesis Review</h3>
      <p>{{ state.left_field }} ↔ {{ state.right_field }} ({{ state.cardinality }}, {{ (state.inclusion_ratio * 100) | number:'1.0-1' }}%)</p>
      <span class="de-badge de-badge-tested">{{ state.epistemic_status }}</span>
      <div class="actions">
        <button type="button" class="btn-ok" (click)="decide('verified')">✓ Approve &amp; Verify</button>
        <button type="button" class="btn-no" (click)="decide('rejected')">✕ Reject (FORBIDDEN_JOIN)</button>
      </div>
    </div>
  `,
  styles: [`
    .panel { padding: 16px; background: #0f172a; color: #f8fafc; height: 100%; }
    .actions { display: flex; gap: 8px; margin-top: 12px; }
    .btn-ok { background: #059669; color: #fff; border: 0; border-radius: 6px; padding: 6px 12px; cursor: pointer; }
    .btn-no { background: #dc2626; color: #fff; border: 0; border-radius: 6px; padding: 6px 12px; cursor: pointer; }
  `],
})
export class JoinHypothesisReviewComponent implements CanvasManagedComponent<JoinHypothesisReviewState> {
  @Input() state: JoinHypothesisReviewState = {
    join_id: 'join-demo-1',
    left_field: 'sales_orders.cust_ref_key',
    right_field: 'crm_customers.party_uuid',
    cardinality: 'N:1',
    inclusion_ratio: 0.97,
    epistemic_status: 'empirically_tested',
  };
  private mountProps: MfeMountProps<JoinHypothesisReviewState> = {};

  initializeFromProps(props: MfeMountProps<JoinHypothesisReviewState>): void {
    this.mountProps = props;
    const incoming = props.initialState ?? props.content;
    if (incoming) this.state = { ...this.state, ...incoming };
  }

  applyPatch(patches: JsonPatchOp[] | Partial<JoinHypothesisReviewState>, _attr?: AiFieldAttribution[]): void {
    this.state = applyJsonPatches(this.state, patches);
  }

  getState(): JoinHypothesisReviewState {
    return JSON.parse(JSON.stringify(this.state));
  }

  getSchema(): Record<string, any> {
    return {
      $schema: 'http://json-schema.org/draft-07/schema#',
      title: 'JoinHypothesisReviewState',
      type: 'object',
      properties: { join_id: { type: 'string' }, epistemic_status: { type: 'string' } },
    };
  }

  decide(status: 'verified' | 'rejected'): void {
    this.state = { ...this.state, epistemic_status: status };
    this.mountProps.onAction?.(status === 'verified' ? 'verify_join' : 'reject_join', this.getState());
  }
}

const wrapper = createCanvasWrapper<JoinHypothesisReviewState, JoinHypothesisReviewComponent>(
  JoinHypothesisReviewComponent,
);
export const mount = wrapper.mount;
export const unmount = wrapper.unmount;
