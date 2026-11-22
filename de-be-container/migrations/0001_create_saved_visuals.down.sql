-- Reversible rollback migration for Spec 13: Saved Data Visual Specifications
DROP INDEX IF EXISTS idx_saved_visuals_family;
DROP INDEX IF EXISTS idx_saved_visuals_dataset;
DROP TABLE IF EXISTS saved_visuals;
