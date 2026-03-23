package handlers

import (
	"context"
	"log/slog"
	"time"

	"connectrpc.com/connect"

	analysisv1 "github.com/org/experimentation/gen/go/experimentation/analysis/v1"
	banditv1 "github.com/org/experimentation/gen/go/experimentation/bandit/v1"

	"github.com/org/experimentation-platform/services/management/internal/store"
)

// submitFdrDecision reads the primary metric's e-value from metric_results and
// submits it to the e-LOND Online FDR controller (ADR-018 Phase 2).
//
// The call is best-effort: any error is logged and silently dropped so that
// experiment conclusion is never blocked by FDR controller failures.
func (s *ExperimentService) submitFdrDecision(ctx context.Context, experimentID, primaryMetricID string) {
	if s.fdrController == nil || s.pool == nil {
		return
	}

	callCtx, cancel := context.WithTimeout(ctx, 3*time.Second)
	defer cancel()

	// Read the most-recently computed e-value for the primary metric.
	// M4a stores results in metric_results after RunAnalysis completes.
	var eValue float64
	err := s.pool.QueryRow(callCtx, `
		SELECT e_value
		FROM   metric_results
		WHERE  experiment_id = $1
		  AND  metric_id     = $2
		  AND  e_value IS NOT NULL
		ORDER  BY computed_at DESC
		LIMIT  1
	`, experimentID, primaryMetricID).Scan(&eValue)
	if err != nil {
		slog.Warn("fdr: primary metric e-value not available, skipping FDR submission",
			"experiment_id", experimentID,
			"metric_id", primaryMetricID,
			"error", err,
		)
		return
	}

	decision, err := s.fdrController.Test(callCtx, experimentID, eValue)
	if err != nil {
		slog.Error("fdr: controller.Test failed",
			"experiment_id", experimentID,
			"e_value", eValue,
			"error", err,
		)
		return
	}

	slog.Info("fdr: experiment submitted to e-LOND controller",
		"experiment_id", experimentID,
		"e_value", eValue,
		"rejected", decision.Rejected,
		"alpha_allocated", decision.AlphaAllocated,
		"wealth_after", decision.WealthAfter,
		"num_tested", decision.NumTested,
	)
}

// analysisTypeForExperiment maps experiment type to the analysis method used at conclude time.
func analysisTypeForExperiment(expType string) string {
	switch expType {
	case "AB", "MULTIVARIATE":
		return "standard"
	case "INTERLEAVING":
		return "interleaving_sign_test_bradley_terry"
	case "SESSION_LEVEL":
		return "clustered_naive_hc1"
	case "PLAYBACK_QOE":
		return "qoe_engagement_correlation"
	case "MAB", "CONTEXTUAL_BANDIT":
		return "ipw_causal"
	case "CUMULATIVE_HOLDOUT":
		return "cumulative_lift_report"
	default:
		return "standard"
	}
}

// handleTypeSpecificConclude dispatches type-specific work during the CONCLUDING
// phase: M4a analysis trigger, M4b policy snapshot (bandits), surrogate projection
// flagging, and analysis type classification. All external calls are best-effort
// with a 5-second timeout so conclude completes even if M4a/M4b are unavailable.
func (s *ExperimentService) handleTypeSpecificConclude(ctx context.Context, exp store.ExperimentRow) map[string]any {
	details := map[string]any{
		"analysis_type": analysisTypeForExperiment(exp.Type),
	}

	// 1. Trigger M4a RunAnalysis (all experiment types, best-effort).
	s.triggerAnalysis(ctx, exp.ExperimentID, details)

	// 2. Capture M4b policy snapshot (MAB/CONTEXTUAL_BANDIT only, best-effort).
	if exp.Type == "MAB" || exp.Type == "CONTEXTUAL_BANDIT" {
		s.capturePolicySnapshot(ctx, exp.ExperimentID, details)
	}

	// 3. Flag surrogate projection if surrogate model is configured.
	if exp.SurrogateModelID != nil && *exp.SurrogateModelID != "" {
		details["surrogate_projection"] = "requested"
		details["surrogate_model_id"] = *exp.SurrogateModelID
	}

	return details
}

// triggerAnalysis calls M4a RunAnalysis with a 5-second timeout.
func (s *ExperimentService) triggerAnalysis(ctx context.Context, experimentID string, details map[string]any) {
	if s.analysisClient == nil {
		details["analysis_trigger"] = "skipped_no_client"
		return
	}

	callCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()

	_, err := s.analysisClient.RunAnalysis(callCtx, connect.NewRequest(&analysisv1.RunAnalysisRequest{
		ExperimentId: experimentID,
	}))
	if err != nil {
		slog.Warn("M4a RunAnalysis failed (best-effort)", "experiment_id", experimentID, "error", err)
		details["analysis_trigger"] = "failed"
		details["analysis_trigger_error"] = err.Error()
		return
	}
	details["analysis_trigger"] = "success"
}

// capturePolicySnapshot calls M4b GetPolicySnapshot with a 5-second timeout.
func (s *ExperimentService) capturePolicySnapshot(ctx context.Context, experimentID string, details map[string]any) {
	if s.banditClient == nil {
		details["policy_snapshot"] = "skipped_no_client"
		return
	}

	callCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()

	resp, err := s.banditClient.GetPolicySnapshot(callCtx, connect.NewRequest(&banditv1.GetPolicySnapshotRequest{
		ExperimentId: experimentID,
	}))
	if err != nil {
		slog.Warn("M4b GetPolicySnapshot failed (best-effort)", "experiment_id", experimentID, "error", err)
		details["policy_snapshot"] = "failed"
		details["policy_snapshot_error"] = err.Error()
		return
	}

	details["policy_snapshot"] = "captured"
	if snap := resp.Msg; snap != nil {
		details["total_rewards_processed"] = snap.TotalRewardsProcessed
		if snap.SnapshotAt != nil {
			details["snapshot_at"] = snap.SnapshotAt.AsTime().Format(time.RFC3339)
		}
	}
}
