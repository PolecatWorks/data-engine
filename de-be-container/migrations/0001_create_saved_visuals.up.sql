-- Reversible forward migration for Spec 13: Saved Data Visual Specifications
CREATE TABLE IF NOT EXISTS saved_visuals (
    id UUID PRIMARY KEY,
    name VARCHAR NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    renderer VARCHAR NOT NULL CHECK (renderer IN ('vega_lite', 'echarts')),
    chart_family VARCHAR NOT NULL,
    primary_dataset_id UUID,
    data_binding JSONB NOT NULL,
    visual_spec JSONB NOT NULL,
    profile_overlays JSONB NOT NULL DEFAULT '{}'::jsonb,
    last_provenance_run_id UUID,
    created_by VARCHAR NOT NULL DEFAULT 'system',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_saved_visuals_dataset ON saved_visuals (primary_dataset_id);
CREATE INDEX IF NOT EXISTS idx_saved_visuals_family ON saved_visuals (chart_family);
