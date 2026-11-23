import { bootstrapApplication } from '@angular/platform-browser';
import { AppComponent } from './app/app.component';
import { appConfig } from './app/app.config';

bootstrapApplication(AppComponent, appConfig).catch((err) => {
  console.error('Application bootstrap error:', err);
  if (typeof document !== 'undefined' && document.body) {
    const banner = document.createElement('div');
    banner.setAttribute('role', 'alert');
    banner.setAttribute('data-testid', 'fatal-bootstrap-error');
    const detail = err instanceof Error ? err.message : String(err);
    banner.textContent = `Data-Engine MFE failed to bootstrap: ${detail}`;
    document.body.appendChild(banner);
  }
  throw err;
});

