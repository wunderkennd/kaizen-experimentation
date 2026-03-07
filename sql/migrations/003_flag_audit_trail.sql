-- ============================================================================
-- Feature Flag Audit Trail: M7 Phase 2
-- Records all mutations to feature flags for compliance and debugging.
-- ============================================================================

CREATE TABLE flag_audit_trail (
    audit_id        UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    flag_id         UUID NOT NULL REFERENCES feature_flags(flag_id) ON DELETE CASCADE,
    action          TEXT NOT NULL CHECK (action IN (
        'create', 'update', 'delete', 'enable', 'disable',
        'rollout_change', 'promote_to_experiment', 'resolve_experiment'
    )),
    actor_email     TEXT NOT NULL DEFAULT 'system',
    previous_value  JSONB DEFAULT '{}',
    new_value       JSONB DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_flag_audit_flag ON flag_audit_trail(flag_id, created_at DESC);
CREATE INDEX idx_flag_audit_action ON flag_audit_trail(action, created_at DESC);

-- ============================================================================
-- Stale flag detection view: flags unchanged for >90 days at 100% rollout.
-- ============================================================================

CREATE VIEW stale_flags AS
SELECT
    f.flag_id,
    f.name,
    f.description,
    f.type,
    f.enabled,
    f.rollout_percentage,
    f.updated_at,
    NOW() - f.updated_at AS stale_duration
FROM feature_flags f
WHERE f.enabled = TRUE
  AND f.rollout_percentage >= 1.0
  AND f.updated_at < NOW() - INTERVAL '90 days'
ORDER BY f.updated_at ASC;
