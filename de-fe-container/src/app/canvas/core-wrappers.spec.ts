import { describe, expect, it, vi, afterEach } from 'vitest';
import { mount as mountAssetForm } from './data-asset-form/data-asset-form.wrapper';
import { mount as mountJoinReview } from './join-hypothesis-review/join-hypothesis-review.wrapper';
import { mount as mountSandbox } from './algorithm-sandbox/algorithm-sandbox.wrapper';
import { mount as mountProvenance } from './provenance-lineage/provenance-lineage.wrapper';

describe('Core Data-Engine Shared Canvas Wrappers (Spec 09)', () => {
  const cleanups: Array<() => void> = [];

  afterEach(() => {
    while (cleanups.length > 0) {
      cleanups.pop()?.();
    }
  });

  function makeHost(): HTMLElement {
    const el = document.createElement('div');
    document.body.appendChild(el);
    cleanups.push(() => el.parentNode?.removeChild(el));
    return el;
  }

  it('mounts DataAssetFormWrapper with MfeCanvasController lifecycle', async () => {
    const host = makeHost();
    const onSelectionChange = vi.fn();
    const ctrl = await mountAssetForm(host, { id: 'asset-1', onSelectionChange });
    cleanups.push(() => ctrl.unmount());

    expect(ctrl.getSchema()['title']).toBe('DataAssetFormState');
    ctrl.applyPatch([{ op: 'replace', path: '/grain_description', value: '1 row per invoice line' }]);
    expect(ctrl.getState().grain_description).toBe('1 row per invoice line');

    const row = host.querySelector('.row') as HTMLElement;
    row.click();
    expect(onSelectionChange).toHaveBeenCalledWith(expect.objectContaining({ canvasId: 'asset-1' }));
  });

  it('mounts JoinHypothesisReviewWrapper and emits verify/reject actions', async () => {
    const host = makeHost();
    const onAction = vi.fn();
    const ctrl = await mountJoinReview(host, { id: 'join-1', onAction });
    cleanups.push(() => ctrl.unmount());

    expect(ctrl.getSchema()['title']).toBe('JoinHypothesisReviewState');
    (host.querySelector('.btn-ok') as HTMLButtonElement).click();
    expect(ctrl.getState().epistemic_status).toBe('verified');
    expect(onAction).toHaveBeenCalledWith('verify_join', expect.objectContaining({ epistemic_status: 'verified' }));
  });

  it('mounts AlgorithmSandboxWrapper and ProvenanceLineageWrapper with JSON Patch support', async () => {
    const host1 = makeHost();
    const sandboxCtrl = await mountSandbox(host1, { id: 'sandbox-1' });
    cleanups.push(() => sandboxCtrl.unmount());
    sandboxCtrl.applyPatch([{ op: 'replace', path: '/language', value: 'sql' }]);
    expect(sandboxCtrl.getState().language).toBe('sql');

    const host2 = makeHost();
    const provCtrl = await mountProvenance(host2, { id: 'prov-1' });
    cleanups.push(() => provCtrl.unmount());
    provCtrl.applyPatch([{ op: 'replace', path: '/all_rules_human_verified', value: false }]);
    expect(provCtrl.getState().all_rules_human_verified).toBe(false);
    expect(host2.textContent).toContain('Contains AI-Inferred Rules');
  });
});
