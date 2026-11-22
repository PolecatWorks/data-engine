import { Routes } from '@angular/router';
import { DataVisualExplorerComponent } from './canvas/data-visual-explorer/data-visual-explorer.component';
import { SemanticTopologyComponent } from './canvas/semantic-topology/semantic-topology.component';
import { DataAssetFormComponent } from './canvas/data-asset-form/data-asset-form.wrapper';
import { JoinHypothesisReviewComponent } from './canvas/join-hypothesis-review/join-hypothesis-review.wrapper';
import { AlgorithmSandboxComponent } from './canvas/algorithm-sandbox/algorithm-sandbox.wrapper';
import { ProvenanceLineageComponent } from './canvas/provenance-lineage/provenance-lineage.wrapper';
import { StatisticalDiagnosticsComponent } from './canvas/statistical-diagnostics/statistical-diagnostics.wrapper';

export const remoteRoutes: Routes = [
  {
    path: '',
    redirectTo: 'explore',
    pathMatch: 'full',
  },
  {
    path: 'explore',
    component: DataVisualExplorerComponent,
    title: 'Data Visual Explorer',
  },
  {
    path: 'explore/:id',
    component: DataVisualExplorerComponent,
    title: 'Saved Data Visual',
  },
  {
    path: 'topology',
    component: SemanticTopologyComponent,
    title: 'Semantic & Structural Topology',
  },
  {
    path: 'datasets',
    component: DataAssetFormComponent,
    title: 'Dataset Steward Form',
  },
  {
    path: 'joins',
    component: JoinHypothesisReviewComponent,
    title: 'Join Hypothesis Review',
  },
  {
    path: 'algorithms',
    component: AlgorithmSandboxComponent,
    title: 'Algorithm Sandbox',
  },
  {
    path: 'provenance',
    component: ProvenanceLineageComponent,
    title: 'Provenance & Lineage',
  },
  {
    path: 'diagnostics',
    component: StatisticalDiagnosticsComponent,
    title: 'Statistical Diagnostics',
  },
  {
    path: '**',
    redirectTo: 'explore',
  },
];

export const routes = remoteRoutes;
export default remoteRoutes;
