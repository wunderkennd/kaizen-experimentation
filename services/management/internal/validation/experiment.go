package validation

import (
	"fmt"
	"math"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// ValidateCreateExperiment validates an experiment for creation.
func ValidateCreateExperiment(exp *commonv1.Experiment) *connect.Error {
	if exp == nil {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment is required"))
	}
	if exp.GetName() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("name is required"))
	}
	if exp.GetOwnerEmail() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("owner_email is required"))
	}
	if exp.GetLayerId() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("layer_id is required"))
	}
	if exp.GetPrimaryMetricId() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("primary_metric_id is required"))
	}
	if exp.GetType() == commonv1.ExperimentType_EXPERIMENT_TYPE_UNSPECIFIED {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("type is required"))
	}

	if err := validateVariants(exp); err != nil {
		return err
	}
	if err := validateTypeConfig(exp); err != nil {
		return err
	}
	return nil
}

// ValidateUpdateExperiment validates an experiment for update.
func ValidateUpdateExperiment(exp *commonv1.Experiment) *connect.Error {
	if exp == nil {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment is required"))
	}
	if exp.GetExperimentId() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("experiment_id is required"))
	}
	return ValidateCreateExperiment(exp)
}

func validateVariants(exp *commonv1.Experiment) *connect.Error {
	variants := exp.GetVariants()
	expType := exp.GetType()

	// Bandits can have fewer than 2 variants (arms defined in BanditConfig).
	isBandit := expType == commonv1.ExperimentType_EXPERIMENT_TYPE_MAB ||
		expType == commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT

	if len(variants) < 2 && !isBandit {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("at least 2 variants required, got %d", len(variants)))
	}
	if len(variants) < 1 {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("at least 1 variant required"))
	}

	// Check variant fields, count controls, sum fractions.
	var fractionSum float64
	controlCount := 0
	for _, v := range variants {
		if v.GetName() == "" {
			return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("variant name is required"))
		}
		if v.GetTrafficFraction() < 0 || v.GetTrafficFraction() > 1 {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("variant %q traffic_fraction must be in [0.0, 1.0], got %f", v.GetName(), v.GetTrafficFraction()))
		}
		fractionSum += v.GetTrafficFraction()
		if v.GetIsControl() {
			controlCount++
		}
	}

	// Traffic fractions must sum to ~1.0 (tolerance for floating point).
	if math.Abs(fractionSum-1.0) > 0.001 {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("traffic fractions must sum to 1.0, got %f", fractionSum))
	}

	// Control variant requirement depends on type.
	requiresControl := expType == commonv1.ExperimentType_EXPERIMENT_TYPE_AB ||
		expType == commonv1.ExperimentType_EXPERIMENT_TYPE_MULTIVARIATE ||
		expType == commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING ||
		expType == commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL ||
		expType == commonv1.ExperimentType_EXPERIMENT_TYPE_PLAYBACK_QOE ||
		expType == commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT

	if requiresControl && controlCount != 1 {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("exactly 1 control variant required for type %s, got %d", expType, controlCount))
	}

	return nil
}

func validateTypeConfig(exp *commonv1.Experiment) *connect.Error {
	switch exp.GetType() {
	case commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING:
		if exp.GetInterleavingConfig() == nil {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("interleaving_config is required for INTERLEAVING type"))
		}
	case commonv1.ExperimentType_EXPERIMENT_TYPE_MAB,
		commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT:
		if exp.GetBanditConfig() == nil {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("bandit_config is required for %s type", exp.GetType()))
		}
	case commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL:
		if exp.GetSessionConfig() == nil {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("session_config is required for SESSION_LEVEL type"))
		}
	}
	return nil
}
