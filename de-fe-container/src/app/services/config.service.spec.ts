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

  it('throws an explicit Error when config.json fetch fails (e.g. HTTP 500)', async () => {
    const svc = new ConfigService();
    const mockFetch = vi.fn(async () => ({
      ok: false,
      status: 500,
      json: async () => ({}),
    })) as unknown as typeof fetch;

    await expect(
      svc.loadConfig(mockFetch, 'https://studio.example.com/mfe/data-engine/main.js'),
    ).rejects.toThrow(/HTTP 500/);
  });

  it('throws an explicit Error when config.json is missing or has an empty apiUrl', async () => {
    const svc = new ConfigService();
    const mockFetchMissing = vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({}),
    })) as unknown as typeof fetch;

    await expect(
      svc.loadConfig(mockFetchMissing, 'https://studio.example.com/mfe/data-engine/main.js'),
    ).rejects.toThrow(/missing required non-empty apiUrl/);

    const mockFetchEmpty = vi.fn(async () => ({
      ok: true,
      status: 200,
      json: async () => ({ apiUrl: '   ' }),
    })) as unknown as typeof fetch;

    await expect(
      svc.loadConfig(mockFetchEmpty, 'https://studio.example.com/mfe/data-engine/main.js'),
    ).rejects.toThrow(/missing required non-empty apiUrl/);
  });

  it('throws an explicit Error when getApiUrlSync() is called before loadConfig()', () => {
    const svc = new ConfigService();
    expect(() => svc.getApiUrlSync()).toThrow(/ConfigService not initialized/);
  });
});

