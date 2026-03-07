package handlers

import (
	"context"
	"fmt"
	"log/slog"
	"strings"

	"connectrpc.com/connect"
	"github.com/google/uuid"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	mgmtv1 "github.com/org/experimentation/gen/go/experimentation/management/v1"
)

// PromoteToExperiment converts a flag to a tracked experiment.
// When a management client is configured, it calls M5's CreateExperiment API.
// Otherwise falls back to a mock response for development/testing.
//
// Type-specific defaults are applied based on experiment_type so the created
// DRAFT experiment is immediately configurable with minimal additional setup.
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
		Variants:           variants,
		PrimaryMetricId:    primaryMetricID,
		SecondaryMetricIds: req.Msg.GetSecondaryMetricIds(),
		TargetingRuleId:    f.TargetingRuleID,
	}

	applyTypeDefaults(experiment, f, primaryMetricID)

	var result *commonv1.Experiment

	if s.managementClient != nil {
		result, err = s.createExperimentViaM5(ctx, experiment)
		if err != nil {
			return nil, err
		}
	} else {
		result = s.createExperimentMock(experiment, f)
	}

	s.recordAudit(ctx, flagID, "promote_to_experiment", f, f)

	return connect.NewResponse(result), nil
}

// applyTypeDefaults sets type-specific config fields on the experiment based
// on the experiment type. These serve as sensible starting points — the user
// can update them via M5's UpdateExperiment before calling StartExperiment.
func applyTypeDefaults(exp *commonv1.Experiment, f *store.Flag, primaryMetricID string) {
	switch exp.Type {
	case commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL:
		exp.SessionConfig = &commonv1.SessionConfig{
			SessionIdAttribute:          "session_id",
			AllowCrossSessionVariation:  true,
			MinSessionsPerUser:          1,
		}

	case commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING:
		// Convert flag variant values to algorithm IDs.
		algorithmIDs := make([]string, 0, len(f.Variants))
		for _, v := range f.Variants {
			algorithmIDs = append(algorithmIDs, v.Value)
		}
		// Need at least 2 algorithm IDs; synthesize if flag had no variants.
		if len(algorithmIDs) < 2 {
			algorithmIDs = []string{"algorithm_control", "algorithm_treatment"}
		}
		exp.InterleavingConfig = &commonv1.InterleavingConfig{
			Method:           commonv1.InterleavingMethod_INTERLEAVING_METHOD_TEAM_DRAFT,
			AlgorithmIds:     algorithmIDs,
			CreditAssignment: commonv1.CreditAssignment_CREDIT_ASSIGNMENT_BINARY_WIN,
			MaxListSize:      50,
		}

	case commonv1.ExperimentType_EXPERIMENT_TYPE_MAB:
		exp.BanditConfig = buildBanditConfig(f, primaryMetricID,
			commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING)

	case commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT:
		exp.BanditConfig = buildBanditConfig(f, primaryMetricID,
			commonv1.BanditAlgorithm_BANDIT_ALGORITHM_LINEAR_UCB)

	case commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT:
		exp.IsCumulativeHoldout = true

	case commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		commonv1.ExperimentType_EXPERIMENT_TYPE_MULTIVARIATE,
		commonv1.ExperimentType_EXPERIMENT_TYPE_PLAYBACK_QOE:
		// No additional type-specific config needed.
	}
}

// buildBanditConfig creates a BanditConfig with arms derived from flag variants.
func buildBanditConfig(f *store.Flag, rewardMetricID string, algorithm commonv1.BanditAlgorithm) *commonv1.BanditConfig {
	arms := make([]*commonv1.BanditArm, 0, len(f.Variants))
	for _, v := range f.Variants {
		arms = append(arms, &commonv1.BanditArm{
			ArmId:       v.VariantID,
			Name:        v.Value,
			PayloadJson: fmt.Sprintf(`{"value": %q}`, v.Value),
		})
	}
	// Synthesize control/treatment arms if flag had no variants.
	if len(arms) < 2 {
		arms = []*commonv1.BanditArm{
			{ArmId: "arm_control", Name: "control", PayloadJson: `{"value": "false"}`},
			{ArmId: "arm_treatment", Name: "treatment", PayloadJson: `{"value": "true"}`},
		}
	}

	return &commonv1.BanditConfig{
		Algorithm:             algorithm,
		Arms:                  arms,
		RewardMetricId:        rewardMetricID,
		MinExplorationFraction: 0.1,
		WarmupObservations:    1000,
	}
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
		"experiment_type", exp.Type.String(),
		"num_variants", len(exp.Variants),
	)

	return exp
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
