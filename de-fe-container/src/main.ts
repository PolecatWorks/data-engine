import { initFederation } from '@angular-architects/native-federation';

initFederation()
  .catch((err) => console.error('Federation init error:', err))
  .then(() => import('./bootstrap'))
  .catch((err) => console.error('Bootstrap error:', err));
