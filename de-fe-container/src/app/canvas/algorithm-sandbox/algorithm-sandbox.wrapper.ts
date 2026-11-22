import { Component, Input } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import {
  AiFieldAttribution,
  CanvasManagedComponent,
  JsonPatchOp,
  MfeMountProps,
  applyJsonPatches,
  createCanvasWrapper,
} from '../mfe-canvas.types';

export interface AlgorithmSandboxState {
  algorithm_id: string;
  language: 'sql' | 'python';
  code: string;
  validation_status: string;
}

@Component({
  selector: 'de-algorithm-sandbox',
  standalone: true,
  imports: [CommonModule, FormsModule],
  template: `
    <div class="panel" data-testid="algorithm-sandbox">
      <h3>Algorithm Sandbox — {{ state.algorithm_id }} ({{ state.language }})</h3>
      <textarea [(ngModel)]="state.code" rows="8"></textarea>
    </div>
  `,
  styles: [`
    .panel { padding: 16px; background: #0f172a; color: #f8fafc; height: 100%; display: flex; flex-direction: column; gap: 10px; }
    textarea { width: 100%; background: #111827; color: #e2e8f0; border: 1px solid #334155; border-radius: 8px; padding: 10px; font-family: monospace; font-size: 12px; }
  `],
})
export class AlgorithmSandboxComponent implements CanvasManagedComponent<AlgorithmSandboxState> {
  @Input() state: AlgorithmSandboxState = {
    algorithm_id: 'cohort_retention_v1',
    language: 'python',
    code: 'def run(inputs, params):\n    return {"rows": len(inputs["orders"])}\n',
    validation_status: 'validated',
  };

  initializeFromProps(props: MfeMountProps<AlgorithmSandboxState>): void {
    const incoming = props.initialState ?? props.content;
    if (incoming) this.state = { ...this.state, ...incoming };
  }

  applyPatch(patches: JsonPatchOp[] | Partial<AlgorithmSandboxState>, _attr?: AiFieldAttribution[]): void {
    this.state = applyJsonPatches(this.state, patches);
  }

  getState(): AlgorithmSandboxState {
    return JSON.parse(JSON.stringify(this.state));
  }

  getSchema(): Record<string, any> {
    return {
      $schema: 'http://json-schema.org/draft-07/schema#',
      title: 'AlgorithmSandboxState',
      type: 'object',
      properties: { algorithm_id: { type: 'string' }, language: { type: 'string' }, code: { type: 'string' } },
    };
  }
}

const wrapper = createCanvasWrapper<AlgorithmSandboxState, AlgorithmSandboxComponent>(AlgorithmSandboxComponent);
export const mount = wrapper.mount;
export const unmount = wrapper.unmount;
