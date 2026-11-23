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
    const response = await fetcher(url);
    if (!response.ok) {
      throw new Error(`Failed to load runtime config from ${url}: HTTP ${response.status}`);
    }
    const parsed = (await response.json()) as Partial<DataEngineRuntimeConfig>;
    if (!parsed || typeof parsed.apiUrl !== 'string' || parsed.apiUrl.trim().length === 0) {
      throw new Error('Invalid runtime config: config.json missing required non-empty apiUrl');
    }
    const normalizedApiUrl = parsed.apiUrl.trim().replace(/\/+$/, '');
    if (normalizedApiUrl.length === 0) {
      throw new Error('Invalid runtime config: config.json apiUrl cannot be empty');
    }
    this.cachedConfig = { apiUrl: normalizedApiUrl };
    return this.cachedConfig;
  }

  getApiUrlSync(): string {
    if (!this.cachedConfig) {
      throw new Error('ConfigService not initialized: loadConfig() must complete before getApiUrlSync()');
    }
    return this.cachedConfig.apiUrl;
  }

  resetForTesting(): void {
    this.cachedConfig = null;
  }
}
