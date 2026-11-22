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

export interface DataAssetFormState {
  dataset_id: string;
  grain_description: string;
  format: string;
  epistemic_status: string;
  fields: Array<{
    field_name: string;
    arrow_data_type: string;
    semantic_label: string;
    unit: string;
    epistemic_status: string;
  }>;
}

@Component({
  selector: 'de-data-asset-form',
  standalone: true,
  imports: [CommonModule, FormsModule],
  template: `
    <div class="panel" data-testid="data-asset-form">
      <h3>Dataset Steward Form — {{ state.dataset_id }}</h3>
      <p class="sub">{{ state.format }} • {{ state.grain_description }}</p>
      <div *ngFor="let f of state.fields; let i = index" class="row" (click)="focusField(f, i)">
        <strong>{{ f.field_name }}</strong>
        <span>{{ f.arrow_data_type }}</span>
        <input [(ngModel)]="f.unit" (ngModelChange)="onFieldEdit(i, 'unit', $event)" placeholder="unit" />
        <span class="de-badge de-badge-tested">{{ f.epistemic_status }}</span>
      </div>
    </div>
  `,
  styles: [`
    .panel { padding: 16px; background: #0f172a; color: #f8fafc; height: 100%; overflow: auto; }
    .sub { color: #94a3b8; font-size: 12px; }
    .row { display: grid; grid-template-columns: 1.5fr 1fr 1fr auto; gap: 8px; align-items: center; padding: 8px; background: #1e293b; border-radius: 6px; margin-bottom: 6px; font-size: 12px; cursor: pointer; }
    input { background: #0f172a; color: #f8fafc; border: 1px solid #334155; border-radius: 4px; padding: 4px 6px; font-size: 12px; }
  `],
})
export class DataAssetFormComponent implements CanvasManagedComponent<DataAssetFormState> {
  @Input() state: DataAssetFormState = {
    dataset_id: 'sales_orders',
    grain_description: '1 row per order',
    format: 'parquet',
    epistemic_status: 'verified',
    fields: [
      { field_name: 'order_id', arrow_data_type: 'Int64', semantic_label: 'Order ID', unit: '', epistemic_status: 'verified' },
      { field_name: 'amount_usd', arrow_data_type: 'Float64', semantic_label: 'Order Total', unit: 'USD', epistemic_status: 'drifted' },
    ],
  };
  private mountProps: MfeMountProps<DataAssetFormState> = {};

  initializeFromProps(props: MfeMountProps<DataAssetFormState>): void {
    this.mountProps = props;
    const incoming = props.initialState ?? props.content;
    if (incoming) this.state = { ...this.state, ...incoming };
  }

  applyPatch(patches: JsonPatchOp[] | Partial<DataAssetFormState>, _attr?: AiFieldAttribution[]): void {
    this.state = applyJsonPatches(this.state, patches);
  }

  getState(): DataAssetFormState {
    return JSON.parse(JSON.stringify(this.state));
  }

  getSchema(): Record<string, any> {
    return {
      $schema: 'http://json-schema.org/draft-07/schema#',
      title: 'DataAssetFormState',
      type: 'object',
      properties: { dataset_id: { type: 'string' }, grain_description: { type: 'string' }, fields: { type: 'array' } },
    };
  }

  focusField(f: DataAssetFormState['fields'][number], idx: number): void {
    this.mountProps.onSelectionChange?.({
      canvasId: this.mountProps.id ?? 'data-asset-form',
      path: `/fields/${idx}`,
      label: `${this.state.dataset_id}.${f.field_name}`,
      context: f,
    });
  }

  onFieldEdit(idx: number, key: string, value: string): void {
    this.mountProps.onStateChange?.(this.getState(), [{ op: 'replace', path: `/fields/${idx}/${key}`, value }]);
  }
}

const wrapper = createCanvasWrapper<DataAssetFormState, DataAssetFormComponent>(DataAssetFormComponent);
export const mount = wrapper.mount;
export const unmount = wrapper.unmount;
