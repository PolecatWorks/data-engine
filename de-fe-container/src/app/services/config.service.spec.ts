import { describe, expect, it, vi } from 'vitest';
import { ConfigService } from './config.service';

describe('ConfigService', () => {
  it('resolves config.json relative to import.meta.url for federated /mfe/data-engine mounts', () => {
    const svc = new ConfigService();
    const resolved = svc.resolveConfigUrl('https://studio.example.com/mfe/data-engine/chunk-abc.js');
    expect(resolved).toBe('https://studio.example.com/mfe/data-engine/config.json');
  });

  it('loads apiUrl from fetched config.json and trims trailing slashes', async () => {
    const svc = new ConfigService();
    const mockFetch = vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ apiUrl: '/api/data-engine/v1///' }),
    })) as unknown as typeof fetch;

    const cfg = await svc.loadConfig(mockFetch, 'https://studio.example.com/mfe/data-engine/main.js');
    expect(mockFetch).toHaveBeenCalledWith('https://studio.example.com/mfe/data-engine/config.json');
    expect(cfg.apiUrl).toBe('/api/data-engine/v1');
    expect(svc.getApiUrlSync()).toBe('/api/data-engine/v1');
  });

  it('falls back cleanly to /api/data-engine/v1 when config.json fetch fails', async () => {
    const svc = new ConfigService();
    const mockFetch = vi.fn(async () => ({
      ok: false,
      status: 404,
      json: async () => ({}),
    })) as unknown as typeof fetch;

    const cfg = await svc.loadConfig(mockFetch, 'https://studio.example.com/mfe/data-engine/main.js');
    expect(cfg.apiUrl).toBe('/api/data-engine/v1');
  });
});
