package handlers

import (
	"context"
	"fmt"
	"log/slog"
	"strings"

	"connectrpc.com/connect"
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"

	"github.com/google/uuid"
)

// PromoteToExperiment converts a flag to a tracked experiment.
// Currently mocked: logs the experiment that would be created and returns a synthetic response.
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

	experimentID := uuid.New().String()

	var variants []*commonv1.Variant
	if len(f.Variants) > 0 {
		for i, v := range f.Variants {
			variants = append(variants, &commonv1.Variant{
				VariantId:       uuid.New().String(),
				Name:            fmt.Sprintf("variant_%d", i),
				TrafficFraction: v.TrafficFraction,
				IsControl:       i == 0,
				PayloadJson:     fmt.Sprintf(`{"value": %q}`, v.Value),
			})
		}
	} else {
		variants = []*commonv1.Variant{
			{
				VariantId:       uuid.New().String(),
				Name:            "control",
				TrafficFraction: 1.0 - f.RolloutPercentage,
				IsControl:       true,
				PayloadJson:     `{"value": "false"}`,
			},
			{
				VariantId:       uuid.New().String(),
				Name:            "treatment",
				TrafficFraction: f.RolloutPercentage,
				IsControl:       false,
				PayloadJson:     `{"value": "true"}`,
			},
		}
	}

	experiment := &commonv1.Experiment{
		ExperimentId:       experimentID,
		Name:               fmt.Sprintf("Promoted from flag: %s", f.Name),
		Description:        fmt.Sprintf("Auto-promoted from feature flag %s (%s)", f.Name, f.FlagID),
		Type:               expType,
		State:              commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT,
		Variants:           variants,
		PrimaryMetricId:    primaryMetricID,
		SecondaryMetricIds: req.Msg.GetSecondaryMetricIds(),
		TargetingRuleId:    f.TargetingRuleID,
		HashSalt:           f.Salt,
	}

	// TODO: Replace with real M5 CreateExperiment call when Agent-5 delivers CRUD.
	slog.Info("PromoteToExperiment (mocked)",
		"flag_id", flagID,
		"flag_name", f.Name,
		"experiment_id", experimentID,
		"experiment_type", expType.String(),
		"primary_metric_id", primaryMetricID,
		"num_variants", len(variants),
	)

	s.recordAudit(ctx, flagID, "promote_to_experiment", f, f)

	return connect.NewResponse(experiment), nil
}
