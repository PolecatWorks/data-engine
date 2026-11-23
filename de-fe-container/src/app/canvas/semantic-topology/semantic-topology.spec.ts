import { describe, expect, it, vi, afterEach } from 'vitest';
import { DataEngineApiService } from '../../services/data-engine-api.service';
import { mount } from './semantic-topology.wrapper';
import { buildCytoscapeElements, defaultSemanticTopologyState } from './semantic-topology.component';

describe('SemanticTopologyWrapper (Spec 15)', () => {
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

  it('builds Cytoscape elements with epistemic, FORBIDDEN_JOIN, drifted, fan_out_warning, CONFOUNDED_BY, and lineage dimming classes', () => {
    const state = defaultSemanticTopologyState();
    state.graph.edges.push(
      {
        id: 'e_forbidden',
        source_id: 'sales_orders.amount_usd',
        target_id: 'crm_customers.party_uuid',
        predicate: 'FORBIDDEN_JOIN',
        epistemic_status: 'rejected',
        confidence: 1.0,
      },
      {
        id: 'e_fanout',
        source_id: 'sales_orders.cust_ref_key',
        target_id: 'crm_customers.segment',
        predicate: 'JOINABLE_WITH',
        cardinality: 'M:N',
        epistemic_status: 'hypothesized',
        confidence: 0.65,
        fan_out_warning: {
          multiplier_estimate: 4.2,
          reason: 'M:N join explodes row count',
          suggested_cte_pattern: 'WITH agg AS (SELECT cust_ref_key, SUM(amount_usd) FROM sales_orders GROUP BY 1)',
        },
      },
    );

    const elements = buildCytoscapeElements(state);
    const driftedNode = elements.find((e) => e.data?.id === 'sales_orders.amount_usd');
    const forbiddenEdge = elements.find((e) => e.data?.id === 'e_forbidden');
    const fanoutEdge = elements.find((e) => e.data?.id === 'e_fanout');
    const confounderEdge = elements.find((e) => e.data?.id === 'e_confounder');

    expect(driftedNode?.classes).toContain('is-drifted');
    expect(forbiddenEdge?.classes).toContain('is-forbidden');
    expect(forbiddenEdge?.data?.['label']).toContain('FORBIDDEN_JOIN');
    expect(fanoutEdge?.classes).toContain('has-fanout');
    expect(fanoutEdge?.data?.['label']).toContain('4.20x');
    expect(confounderEdge?.classes).toContain('is-confounder');

    // ER lens hides Layer 2 business concepts
    state.lens = 'er_join';
    const erElements = buildCytoscapeElements(state);
    expect(erElements.find((e) => e.data?.id === 'net_revenue')).toBeUndefined();

    // Lineage overlay highlights target path and dims unrelated nodes
    state.lens = 'lineage_overlay';
    state.highlighted_path = {
      node_ids: ['net_revenue', 'sales_orders.amount_usd'],
      edge_ids: ['e_metric_orders'],
    };
    const lineageElements = buildCytoscapeElements(state);
    expect(lineageElements.find((e) => e.data?.id === 'net_revenue')?.classes).toContain('is-highlighted');
    expect(lineageElements.find((e) => e.data?.id === 'crm_customers.segment')?.classes).toContain('is-dimmed');
  });

  it('mounts MfeCanvasController, handles degraded GraphRAG banner, switches lenses via patch, and emits steward & explore actions', async () => {
    host = document.createElement('div');
    document.body.appendChild(host);

    const onSelectionChange = vi.fn();
    const onAction = vi.fn();

    const initial = defaultSemanticTopologyState();
    initial.graph.graphrag_overlay_degraded = true;

    const controller = await mount(host, {
      id: 'canvas-topo-1',
      initialState: initial,
      onSelectionChange,
      onAction,
    });
    unmountFn = () => controller.unmount();

    const rootBefore = host.querySelector('[data-testid="semantic-topology"]');
    expect(rootBefore).not.toBeNull();
    expect(host.querySelector('[data-testid="degraded-overlay-banner"]')?.textContent).toContain(
      'GraphRAG Layer 2 semantic overlay is temporarily unavailable',
    );

    // Patch lens to field_anatomy and highlight path in-place
    controller.applyPatch([
      { op: 'replace', path: '/lens', value: 'field_anatomy' },
      {
        op: 'replace',
        path: '/highlighted_path',
        value: {
          node_ids: ['sales_orders.cust_ref_key', 'crm_customers.party_uuid'],
          edge_ids: ['join_sales_orders_cust_ref_key__crm_customers_party_uuid'],
        },
      },
    ]);

    expect(host.querySelector('[data-testid="semantic-topology"]')).toBe(rootBefore);
    expect(controller.getState().lens).toBe('field_anatomy');
    expect(host.querySelector('[data-testid="overlap-venn-strip"]')?.textContent).toContain('Jaccard 94%');

    // Click edge row to verify onSelectionChange
    const edgeBtn = host.querySelector(
      '[data-testid="edge-join_sales_orders_cust_ref_key__crm_customers_party_uuid"]',
    ) as HTMLButtonElement;
    edgeBtn.click();
    expect(onSelectionChange).toHaveBeenCalledWith(
      expect.objectContaining({
        canvasId: 'canvas-topo-1',
        id: 'join_sales_orders_cust_ref_key__crm_customers_party_uuid',
      }),
    );

    // Approve & Verify steward action
    const stewardSpy = vi
      .spyOn(DataEngineApiService.prototype, 'stewardJoin')
      .mockResolvedValue(undefined as never);
    try {
      const approveBtn = host.querySelector('[data-testid="btn-approve-join"]') as HTMLButtonElement;
      approveBtn.click();
      await vi.waitFor(() =>
        expect(onAction).toHaveBeenCalledWith(
          'verify_join',
          expect.objectContaining({
            join_id: 'join_sales_orders_cust_ref_key__crm_customers_party_uuid',
            epistemic_status: 'verified',
          }),
        ),
      );
    } finally {
      stewardSpy.mockRestore();
    }

    // Explore Joined Data pivot action
    const exploreBtn = host.querySelector('[data-testid="btn-explore-joined-data"]') as HTMLButtonElement;
    exploreBtn.click();
    expect(onAction).toHaveBeenCalledWith(
      'explore_data_visual',
      expect.objectContaining({
        left_dataset: 'sales_orders',
        right_dataset: 'crm_customers',
      }),
    );
  });
});
