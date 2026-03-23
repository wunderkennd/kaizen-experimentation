-- ============================================================================
-- ADR-018 Phase 1: Add e-value columns to metric_results
--
-- e_value      — the GROW or AVLM e-value E_n (nonneg, E[E] ≤ 1 under H0).
--                NULL when e-value testing is not configured for the metric.
-- log_e_value  — natural log of e_value; stored separately for numerical
--                stability when E_n is very large or near zero.
-- ============================================================================

ALTER TABLE metric_results
    ADD COLUMN IF NOT EXISTS e_value     DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS log_e_value DOUBLE PRECISION;

COMMENT ON COLUMN metric_results.e_value IS
    'GROW or AVLM mixture e-value (ADR-018). NULL when not computed.';
COMMENT ON COLUMN metric_results.log_e_value IS
    'Natural log of e_value; numerically stable for very large/small e-values.';
