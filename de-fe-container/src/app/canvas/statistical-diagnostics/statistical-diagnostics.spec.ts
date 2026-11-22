import { describe, expect, it, vi, afterEach } from 'vitest';
import { mount, defaultStatisticalDiagnosticsState } from './statistical-diagnostics.wrapper';

describe('StatisticalDiagnosticsWrapper (Spec 12)', () => {
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

  it('renders assumption check scorecard, recommended alternative, sampling disclosure, and verifies traits', async () => {
    host = document.createElement('div');
    document.body.appendChild(host);

    const onAction = vi.fn();
    const initial = defaultStatisticalDiagnosticsState();
    initial.sample_context = {
      population_rows: 500_000,
      analyzed_rows: 100_000,
      sampled: true,
      sampling_seed: 99,
    };

    const controller = await mount(host, {
      id: 'canvas-stat-1',
      initialState: initial,
      onAction,
    });
    unmountFn = () => controller.unmount();

    expect(host.querySelector('[data-testid="stat-sampling-banner"]')?.textContent).toContain('seed=99');
    expect(host.querySelector('[data-testid="assumption-scorecard"]')?.textContent).toContain('failed');
    expect(host.querySelector('[data-testid="assumption-scorecard"]')?.textContent).toContain('passed');
    expect(host.querySelector('[data-testid="recommended-alternative"]')?.textContent).toContain('mann_whitney_u');

    const verifyBtn = host.querySelector('[data-testid="btn-verify-trait"]') as HTMLButtonElement;
    verifyBtn.click();

    expect(onAction).toHaveBeenCalledWith('verify_statistical_trait', {
      field_canonical_id: 'sales_orders.order_total_usd',
    });
    expect(controller.getState().learned_field_traits[0].epistemic_status).toBe('verified');
  });
});
