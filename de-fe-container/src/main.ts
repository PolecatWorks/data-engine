import { initFederation } from '@angular-architects/native-federation';

function renderFatalBootstrapError(message: string): void {
  if (typeof document !== 'undefined' && document.body) {
    const banner = document.createElement('div');
    banner.setAttribute('role', 'alert');
    banner.setAttribute('data-testid', 'fatal-bootstrap-error');
    banner.textContent = message;
    document.body.appendChild(banner);
  }
}

initFederation()
  .then(() => import('./bootstrap'))
  .catch((err) => {
    console.error('Fatal bootstrap error:', err);
    const detail = err instanceof Error ? err.message : String(err);
    renderFatalBootstrapError(`Data-Engine MFE failed to initialize: ${detail}`);
  });

