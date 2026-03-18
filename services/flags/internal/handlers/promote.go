package handlers

import (
	"context"
	"fmt"
	"log/slog"
	"strings"

	"connectrpc.com/connect"
	"github.com/google/uuid"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"github.com/org/experimentation-platform/services/flags/internal/store"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
)

// PromoteToExperiment converts a flag to a tracked experiment.
// When a management client is configured, it calls M5's CreateExperiment API.
// Otherwise falls back to a mock response for development/testing.
func (s *FlagService) PromoteToExperiment(ctx context.Context, req *connect.Request[flagsv1.PromoteToExperimentRequest]) (*connect.Response[commonv1.Experiment], error) {
	flagID := req.Msg.GetFlagId()
	if flagID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("flag_id is required"))
	}

	expType := req.Msg.GetExperimentType()
	if expType == commonv1.ExperimentType_EXPERIMENT_TYPE_UNSPECIFIED {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_type is required"))
	}

	primaryMetricID := req.Msg.GetPrimaryMetricId()
	if primaryMetricID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("primary_metric_id is required"))
	}

	f, err := s.store.GetFlag(ctx, flagID)
	if err != nil {
		if strings.Contains(err.Error(), "not found") {
			return nil, connect.NewError(connect.CodeNotFound, err)
		}
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("get flag: %w", err))
	}

	if !f.Enabled {
		return nil, connect.NewError(connect.CodeFailedPrecondition, fmt.Errorf("flag must be enabled to promote"))
	}

	variants := buildVariants(f)
	actor := actorFromContext(ctx)

	experiment := &commonv1.Experiment{
		Name:               fmt.Sprintf("Promoted from flag: %s", f.Name),
		Description:        fmt.Sprintf("Auto-promoted from feature flag %s (%s)", f.Name, f.FlagID),
		OwnerEmail:         actor,
		Type:               expType,
		LayerId:            s.defaultLayerID,
		Variants:           variants,
		PrimaryMetricId:    primaryMetricID,
		SecondaryMetricIds: req.Msg.GetSecondaryMetricIds(),
		TargetingRuleId:    f.TargetingRuleID,
	}

	if err := applyTypeConfig(experiment, f); err != nil {
		return nil, err
	}

	var result *commonv1.Experiment

	if s.managementClient != nil {
		result, err = s.createExperimentViaM5(ctx, experiment)
		if err != nil {
			if s.metrics != nil {
				s.metrics.FlagPromotionsTotal.Add(ctx, 1, metric.WithAttributes(
					attribute.String("experiment_type", expType.String()),
					attribute.String("status", "error"),
				))
			}
			return nil, err
		}
	} else {
		result = s.createExperimentMock(experiment, f)
	}

	if s.metrics != nil {
		s.metrics.FlagPromotionsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("experiment_type", expType.String()),
			attribute.String("status", "success"),
		))
	}

	// Link the flag to the created experiment for lifecycle tracking.
	if experimentID := result.GetExperimentId(); experimentID != "" {
		if linkErr := s.store.LinkFlagToExperiment(ctx, flagID, experimentID); linkErr != nil {
			slog.Error("failed to link flag to experiment (non-fatal)",
				"error", linkErr, "flag_id", flagID, "experiment_id", experimentID)
		}
	}

	s.recordAudit(ctx, flagID, "promote_to_experiment", f, f)

	return connect.NewResponse(result), nil
}

// createExperimentViaM5 calls Agent-5's real CreateExperiment API.
// If the call fails, the flag state is unchanged (atomic guarantee).
func (s *FlagService) createExperimentViaM5(ctx context.Context, exp *commonv1.Experiment) (*commonv1.Experiment, error) {
	resp, err := s.managementClient.CreateExperiment(ctx, connect.NewRequest(&mgmtv1.CreateExperimentRequest{
		Experiment: exp,
	}))
	if err != nil {
		slog.Error("M5 CreateExperiment failed",
			"error", err,
			"experiment_name", exp.GetName(),
		)
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("create experiment in M5: %w", err))
	}

	slog.Info("PromoteToExperiment succeeded via M5",
		"experiment_id", resp.Msg.GetExperimentId(),
		"experiment_name", resp.Msg.GetName(),
		"state", resp.Msg.GetState().String(),
	)

	return resp.Msg, nil
}

// createExperimentMock returns a synthetic experiment for development/testing
// when no management client is configured.
func (s *FlagService) createExperimentMock(exp *commonv1.Experiment, f *store.Flag) *commonv1.Experiment {
	exp.ExperimentId = uuid.New().String()
	exp.State = commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT
	exp.HashSalt = f.Salt

	for _, v := range exp.Variants {
		if v.VariantId == "" {
			v.VariantId = uuid.New().String()
		}
	}

	slog.Info("PromoteToExperiment (mocked — no management client configured)",
		"flag_id", f.FlagID,
		"flag_name", f.Name,
		"experiment_id", exp.ExperimentId,
		"num_variants", len(exp.Variants),
	)

	return exp
}

// applyTypeConfig sets type-specific experiment configuration based on ExperimentType.
func applyTypeConfig(exp *commonv1.Experiment, f *store.Flag) error {
	switch exp.Type {
	case commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		commonv1.ExperimentType_EXPERIMENT_TYPE_MULTIVARIATE,
		commonv1.ExperimentType_EXPERIMENT_TYPE_PLAYBACK_QOE:
		// No additional config needed.

	case commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING:
		if len(f.Variants) < 2 {
			return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("interleaving requires at least 2 variants (algorithm_ids)"))
		}
		algorithmIDs := make([]string, len(f.Variants))
		for i, v := range f.Variants {
			algorithmIDs[i] = v.Value
		}
		exp.InterleavingConfig = &commonv1.InterleavingConfig{
			Method:           commonv1.InterleavingMethod_INTERLEAVING_METHOD_TEAM_DRAFT,
			AlgorithmIds:     algorithmIDs,
			MaxListSize:      50,
			CreditAssignment: commonv1.CreditAssignment_CREDIT_ASSIGNMENT_BINARY_WIN,
		}

	case commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL:
		exp.SessionConfig = &commonv1.SessionConfig{
			SessionIdAttribute:          "session_id",
			AllowCrossSessionVariation:  true,
		}

	case commonv1.ExperimentType_EXPERIMENT_TYPE_MAB:
		if len(f.Variants) < 2 {
			return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("MAB requires at least 2 variants (arms)"))
		}
		exp.BanditConfig = buildBanditConfig(commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING, f)

	case commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT:
		if len(f.Variants) < 2 {
			return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("contextual bandit requires at least 2 variants (arms)"))
		}
		exp.BanditConfig = buildBanditConfig(commonv1.BanditAlgorithm_BANDIT_ALGORITHM_LINEAR_UCB, f)

	case commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT:
		exp.IsCumulativeHoldout = true
	}

	return nil
}

// buildBanditConfig creates a BanditConfig from flag variants.
func buildBanditConfig(algo commonv1.BanditAlgorithm, f *store.Flag) *commonv1.BanditConfig {
	arms := make([]*commonv1.BanditArm, len(f.Variants))
	for i, v := range f.Variants {
		arms[i] = &commonv1.BanditArm{
			ArmId:       v.VariantID,
			Name:        v.Value,
			PayloadJson: fmt.Sprintf(`{"value": %q}`, v.Value),
		}
	}
	return &commonv1.BanditConfig{
		Algorithm:              algo,
		Arms:                   arms,
		MinExplorationFraction: 0.1,
	}
}

// buildVariants creates experiment variants from a flag's configuration.
func buildVariants(f *store.Flag) []*commonv1.Variant {
	if len(f.Variants) > 0 {
		variants := make([]*commonv1.Variant, len(f.Variants))
		for i, v := range f.Variants {
			variants[i] = &commonv1.Variant{
				Name:            fmt.Sprintf("variant_%d", i),
				TrafficFraction: v.TrafficFraction,
				IsControl:       i == 0,
				PayloadJson:     fmt.Sprintf(`{"value": %q}`, v.Value),
			}
		}
		return variants
	}

	return []*commonv1.Variant{
		{
			Name:            "control",
			TrafficFraction: 1.0 - f.RolloutPercentage,
			IsControl:       true,
			PayloadJson:     `{"value": "false"}`,
		},
		{
			Name:            "treatment",
			TrafficFraction: f.RolloutPercentage,
			IsControl:       false,
			PayloadJson:     `{"value": "true"}`,
		},
	}
}
