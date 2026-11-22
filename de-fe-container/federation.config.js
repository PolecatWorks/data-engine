const { withNativeFederation, shareAll } = require('@angular-architects/native-federation/config');

module.exports = withNativeFederation({
  name: 'data-engine',

  exposes: {
    './routes': './src/app/remote.routes.ts',
    './DataAssetFormWrapper': './src/app/canvas/data-asset-form/data-asset-form.wrapper.ts',
    './JoinHypothesisReviewWrapper': './src/app/canvas/join-hypothesis-review/join-hypothesis-review.wrapper.ts',
    './AlgorithmSandboxWrapper': './src/app/canvas/algorithm-sandbox/algorithm-sandbox.wrapper.ts',
    './ProvenanceLineageWrapper': './src/app/canvas/provenance-lineage/provenance-lineage.wrapper.ts',
    './StatisticalDiagnosticsWrapper': './src/app/canvas/statistical-diagnostics/statistical-diagnostics.wrapper.ts',
    './DataVisualExplorerWrapper': './src/app/canvas/data-visual-explorer/data-visual-explorer.wrapper.ts',
    './SemanticTopologyWrapper': './src/app/canvas/semantic-topology/semantic-topology.wrapper.ts',
  },

  shared: {
    ...shareAll({ singleton: true, strictVersion: false, requiredVersion: 'auto' }),
  },

  skip: [
    'rxjs/ajax',
    'rxjs/fetch',
    'rxjs/testing',
    'rxjs/webSocket',
    'vega',
    'vega-lite',
    'vega-embed',
    'echarts',
    'cytoscape',
    'cytoscape-elk',
    'elkjs',
    'elkjs/lib/elk.bundled.js',
  ],

  features: {
    ignoreUnusedDeps: true,
  },
});
