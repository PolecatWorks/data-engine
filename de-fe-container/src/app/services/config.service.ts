import { Injectable } from '@angular/core';

export interface DataEngineRuntimeConfig {
  apiUrl: string;
}

@Injectable({ providedIn: 'root' })
export class ConfigService {
  private cachedConfig: DataEngineRuntimeConfig | null = null;

  /**
   * Resolves config.json relative to this module's URL (`import.meta.url`) so the MFE
   * always fetches `/mfe/data-engine/config.json` when federated inside Mfe-shell or Angry-agent.
   */
  resolveConfigUrl(baseUrl: string = import.meta.url): string {
    return new URL('config.json', baseUrl).toString();
  }

  async loadConfig(fetcher: typeof fetch = fetch, baseUrl: string = import.meta.url): Promise<DataEngineRuntimeConfig> {
    if (this.cachedConfig) {
      return this.cachedConfig;
    }
    const url = this.resolveConfigUrl(baseUrl);
    try {
      const response = await fetcher(url);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const parsed = (await response.json()) as Partial<DataEngineRuntimeConfig>;
      if (!parsed.apiUrl || typeof parsed.apiUrl !== 'string') {
        throw new Error('config.json missing required apiUrl');
      }
      this.cachedConfig = { apiUrl: parsed.apiUrl.replace(/\/+$/, '') };
      return this.cachedConfig;
    } catch {
      this.cachedConfig = { apiUrl: '/api/data-engine/v1' };
      return this.cachedConfig;
    }
  }

  getApiUrlSync(): string {
    return this.cachedConfig?.apiUrl ?? '/api/data-engine/v1';
  }

  resetForTesting(): void {
    this.cachedConfig = null;
  }
}
