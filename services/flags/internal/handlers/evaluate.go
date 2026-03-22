package handlers

import (
	"context"
	"fmt"
	"strings"

	"connectrpc.com/connect"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"github.com/org/experimentation-platform/services/flags/internal/hash"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
)

// evalOutcome classifies a flag evaluation result for metrics.
type evalOutcome string

const (
	outcomeDefault   evalOutcome = "default"
	outcomeTreatment evalOutcome = "treatment"
	outcomeControl   evalOutcome = "control"
)

func (s *FlagService) EvaluateFlag(ctx context.Context, req *connect.Request[flagsv1.EvaluateFlagRequest]) (*connect.Response[flagsv1.EvaluateFlagResponse], error) {
	flagID := req.Msg.GetFlagId()
	userID := req.Msg.GetUserId()

	if flagID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("flag_id is required"))
	}
	if userID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("user_id is required"))
	}

	f, err := s.store.GetFlag(ctx, flagID)
	if err != nil {
		if strings.Contains(err.Error(), "not found") {
			return nil, connect.NewError(connect.CodeNotFound, err)
		}
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("get flag: %w", err))
	}

	value, variantID, outcome := evaluateFlag(f, userID)

	if s.metrics != nil {
		s.metrics.FlagEvaluationsTotal.Add(ctx, 1, metric.WithAttributes(
			attribute.String("result", string(outcome)),
		))
	}

	return connect.NewResponse(&flagsv1.EvaluateFlagResponse{
		FlagId:    flagID,
		Value:     value,
		VariantId: variantID,
	}), nil
}

func (s *FlagService) EvaluateFlags(ctx context.Context, req *connect.Request[flagsv1.EvaluateFlagsRequest]) (*connect.Response[flagsv1.EvaluateFlagsResponse], error) {
	userID := req.Msg.GetUserId()
	if userID == "" {
		return nil, connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("user_id is required"))
	}

	flags, err := s.store.GetAllEnabledFlags(ctx)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("get flags: %w", err))
	}

	resp := &flagsv1.EvaluateFlagsResponse{}
	for _, f := range flags {
		value, variantID, outcome := evaluateFlag(f, userID)
		if s.metrics != nil {
			s.metrics.FlagEvaluationsTotal.Add(ctx, 1, metric.WithAttributes(
				attribute.String("result", string(outcome)),
			))
		}
		resp.Evaluations = append(resp.Evaluations, &flagsv1.EvaluateFlagResponse{
			FlagId:    f.FlagID,
			Value:     value,
			VariantId: variantID,
		})
	}

	return connect.NewResponse(resp), nil
}

// evaluateFlag determines the flag value for a given user and returns the evaluation outcome.
func evaluateFlag(f *store.Flag, userID string) (value string, variantID string, outcome evalOutcome) {
	if !f.Enabled {
		return f.DefaultValue, "", outcomeDefault
	}

	bucket := hash.Bucket(userID, f.Salt, 10000)
	threshold := uint32(f.RolloutPercentage * 10000)

	if bucket >= threshold {
		return f.DefaultValue, "", outcomeControl
	}

	// User is in rollout.
	if len(f.Variants) == 0 {
		if f.Type == "BOOLEAN" {
			return "true", "", outcomeTreatment
		}
		return f.DefaultValue, "", outcomeTreatment
	}

	// Multi-variant: assign based on cumulative traffic_fraction.
	var cumulative float64
	bucketFraction := float64(bucket) / 10000.0
	for _, v := range f.Variants {
		cumulative += v.TrafficFraction
		if bucketFraction < cumulative {
			return v.Value, v.VariantID, outcomeTreatment
		}
	}

	// Fallback to last variant (handles float rounding).
	last := f.Variants[len(f.Variants)-1]
	return last.Value, last.VariantID, outcomeTreatment
}
