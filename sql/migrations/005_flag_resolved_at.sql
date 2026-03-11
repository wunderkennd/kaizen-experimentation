-- ============================================================================
-- Flag Resolution Tracking: M7 Phase 5
-- Tracks when a promoted flag was resolved (auto or manual),
-- preventing the reconciler from re-processing already-resolved flags.
-- ============================================================================

ALTER TABLE feature_flags
    ADD COLUMN IF NOT EXISTS resolved_at TIMESTAMPTZ;
