import { describe, expect, it } from 'vitest';
import { remoteRoutes } from './remote.routes';

describe('Data-Engine Native Federation Routes & Exposes', () => {
  it('registers Studio routes for /explore, /explore/:id, /topology, and steward views', () => {
    const paths = remoteRoutes.map((r) => r.path);
    expect(paths).toContain('explore');
    expect(paths).toContain('explore/:id');
    expect(paths).toContain('topology');
    expect(paths).toContain('datasets');
    expect(paths).toContain('joins');
    expect(paths).toContain('algorithms');
    expect(paths).toContain('provenance');
    expect(paths).toContain('diagnostics');
  });

  it('uses relative route paths without leading slashes for safe Mfe-shell embedding', () => {
    for (const route of remoteRoutes) {
      expect(route.path?.startsWith('/')).toBe(false);
    }
  });
});
