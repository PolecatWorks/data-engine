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

export interface ProvenanceLineageState {
  provenance_run_id: string;
  all_rules_human_verified: boolean;
  signature_sha256: string;
  mermaid_lineage: string;
}

@Component({
  selector: 'de-provenance-lineage',
  standalone: true,
  imports: [CommonModule],
  template: `
    <div class="panel" data-testid="provenance-lineage">
      <h3>Provenance &amp; Decision Lineage — {{ state.provenance_run_id }}</h3>
      <span class="de-badge" [ngClass]="state.all_rules_human_verified ? 'de-badge-verified' : 'de-badge-drifted'">
        {{ state.all_rules_human_verified ? '✓ All Rules Human-Verified' : '⚠ Contains AI-Inferred Rules' }}
      </span>
      <pre class="dag">{{ state.mermaid_lineage }}</pre>
    </div>
  `,
  styles: [`
    .panel { padding: 16px; background: #0f172a; color: #f8fafc; height: 100%; overflow: auto; }
    .dag { margin-top: 10px; background: #111827; padding: 12px; border-radius: 8px; border: 1px solid #1f2937; font-size: 12px; overflow: auto; }
  `],
})
export class ProvenanceLineageComponent implements CanvasManagedComponent<ProvenanceLineageState> {
  @Input() state: ProvenanceLineageState = {
    provenance_run_id: 'prov-demo-001',
    all_rules_human_verified: true,
    signature_sha256: 'e3b0c44298fc1c14',
    mermaid_lineage: 'flowchart LR\n    DS0["sales_orders"] --> Engine["Data-Engine Safe Executor"]\n    Engine --> Out["Net Revenue"]',
  };

  initializeFromProps(props: MfeMountProps<ProvenanceLineageState>): void {
    const incoming = props.initialState ?? props.content;
    if (incoming) this.state = { ...this.state, ...incoming };
  }

  applyPatch(patches: JsonPatchOp[] | Partial<ProvenanceLineageState>, _attr?: AiFieldAttribution[]): void {
    this.state = applyJsonPatches(this.state, patches);
  }

  getState(): ProvenanceLineageState {
    return JSON.parse(JSON.stringify(this.state));
  }

  getSchema(): Record<string, any> {
    return {
      $schema: 'http://json-schema.org/draft-07/schema#',
      title: 'ProvenanceLineageState',
      type: 'object',
      properties: { provenance_run_id: { type: 'string' }, mermaid_lineage: { type: 'string' } },
    };
  }
}

const wrapper = createCanvasWrapper<ProvenanceLineageState, ProvenanceLineageComponent>(ProvenanceLineageComponent);
export const mount = wrapper.mount;
export const unmount = wrapper.unmount;
