import { describe, expect, it, vi, afterEach } from 'vitest';
import { mount } from './data-visual-explorer.wrapper';
import { defaultDataVisualExplorerState } from './data-visual-explorer.component';

describe('DataVisualExplorerWrapper (Spec 15)', () => {
  let host: HTMLElement | null = null;
  let unmountFn: (() => void) | null = null;

  afterEach(() => {
    if (unmountFn) {
      unmountFn();
      unmountFn = null;
    }
    if (host && host.parentNode) {
      host.parentNode.removeChild(host);
    }
    host = null;
  });

  it('renders Verified vs AI-Inferred trust badges, sampling disclosure banner, and supports in-place JSON Patch without unmounting DOM', async () => {
    host = document.createElement('div');
    document.body.appendChild(host);

    const onSelectionChange = vi.fn();
    const onAction = vi.fn();
    const onStateChange = vi.fn();

    const initialState = defaultDataVisualExplorerState();
    initialState.provenance = {
      provenance_run_id: 'prov-test-1',
      trust_summary: {
        all_rules_human_verified: false,
        min_rule_confidence: 0.78,
        verified_by_stewards: [],
        pre_aggregation_applied: true,
        unit_conversion_applied: false,
      },
      sampled: true,
      population_rows: 1_240_000,
      rendered_rows: 10_000,
      sampling_seed: 42,
    };

    const controller = await mount(host, {
      id: 'canvas-vis-1',
      initialState,
      onSelectionChange,
      onAction,
      onStateChange,
    });
    unmountFn = () => controller.unmount();

    const rootElBefore = host.querySelector('[data-testid="data-visual-explorer"]');
    expect(rootElBefore).not.toBeNull();
    expect(host.querySelector('[data-testid="trust-badge"]')?.textContent).toContain('AI-Inferred');
    expect(host.querySelector('[data-testid="sampling-disclosure-banner"]')?.textContent).toContain('seed=42');

    // Verify getSchema() exposes Vega-Lite/ECharts renderer and expected actions
    const schema = controller.getSchema();
    expect(schema['title']).toBe('DataVisualExplorerState');
    expect(schema['properties']['renderer']['enum']).toEqual(['vega_lite', 'echarts']);
    expect(schema['x-actions']).toContain('run_statistical_check');

    // Apply RFC 6902 JSON Patches: switch Y-axis to log scale, add annotation, verify provenance
    controller.applyPatch(
      [
        { op: 'replace', path: '/spec/encoding/y/scale/type', value: 'log' },
        {
          op: 'add',
          path: '/profile_overlays/annotations/-',
          value: { x: 'Enterprise', label: 'Promo Spike' },
        },
        { op: 'replace', path: '/provenance/trust_summary/all_rules_human_verified', value: true },
      ],
      [{ path: '/spec/encoding/y/scale/type', source: 'ai', confidence: 0.94, rationale: 'Heavy-tailed distribution' }],
    );

    // Same root DOM element must remain mounted (in-place patch, no unmount/remount)
    const rootElAfter = host.querySelector('[data-testid="data-visual-explorer"]');
    expect(rootElAfter).toBe(rootElBefore);

    const patched = controller.getState();
    expect(patched.spec['encoding']['y']['scale']['type']).toBe('log');
    expect(patched.profile_overlays.annotations).toHaveLength(1);
    expect(patched.profile_overlays.annotations[0].label).toBe('Promo Spike');
    expect(host.querySelector('[data-testid="trust-badge"]')?.textContent).toContain('Verified Provenance');
    expect(host.querySelector('[data-testid="annotations-bar"]')?.textContent).toContain('Promo Spike');
    expect(host.querySelector('[data-testid="ai-attribution-badge"]')?.textContent).toContain('AI Patched (1)');

    // Toggle renderer between Vega-Lite and Apache ECharts
    const toggleBtn = host.querySelector('[data-testid="btn-toggle-renderer"]') as HTMLButtonElement;
    toggleBtn.click();
    expect(controller.getState().renderer).toBe('echarts');
    expect(onStateChange).toHaveBeenCalled();

    // Click datum row to verify onSelectionChange
    const firstRow = host.querySelector('tbody tr') as HTMLTableRowElement;
    firstRow.click();
    expect(onSelectionChange).toHaveBeenCalledWith(
      expect.objectContaining({
        canvasId: 'canvas-vis-1',
        path: '/selection/datum/0',
      }),
    );

    // Trigger Run Statistical Check action
    const statBtn = host.querySelector('[data-testid="btn-run-stat-check"]') as HTMLButtonElement;
    statBtn.click();
    expect(onAction).toHaveBeenCalledWith(
      'run_statistical_check',
      expect.objectContaining({
        columns: expect.arrayContaining(['segment', 'net_revenue']),
      }),
    );
  });
});
