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
		if err := validateInterleavingConfig(exp); err != nil {
			return err
		}
	case commonv1.ExperimentType_EXPERIMENT_TYPE_MAB:
		if err := validateBanditConfig(exp, false); err != nil {
			return err
		}
	case commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT:
		if err := validateBanditConfig(exp, true); err != nil {
			return err
		}
	case commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL:
		if err := validateSessionConfig(exp); err != nil {
			return err
		}
	case commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT:
		if !exp.GetIsCumulativeHoldout() {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("is_cumulative_holdout must be true for CUMULATIVE_HOLDOUT type"))
		}
	}
	return nil
}

func validateInterleavingConfig(exp *commonv1.Experiment) *connect.Error {
	cfg := exp.GetInterleavingConfig()
	if cfg == nil {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("interleaving_config is required for INTERLEAVING type"))
	}
	if cfg.GetMethod() == commonv1.InterleavingMethod_INTERLEAVING_METHOD_UNSPECIFIED {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("interleaving_config.method must not be UNSPECIFIED"))
	}

	ids := cfg.GetAlgorithmIds()
	minAlgorithms := 2
	if cfg.GetMethod() == commonv1.InterleavingMethod_INTERLEAVING_METHOD_MULTILEAVE {
		minAlgorithms = 3
	}
	if len(ids) < minAlgorithms {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("interleaving_config.algorithm_ids must have >= %d entries for %s, got %d",
				minAlgorithms, cfg.GetMethod(), len(ids)))
	}
	for i, id := range ids {
		if id == "" {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("interleaving_config.algorithm_ids[%d] must be non-empty", i))
		}
	}
	return nil
}

func validateBanditConfig(exp *commonv1.Experiment, isContextual bool) *connect.Error {
	cfg := exp.GetBanditConfig()
	if cfg == nil {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("bandit_config is required for %s type", exp.GetType()))
	}
	if cfg.GetAlgorithm() == commonv1.BanditAlgorithm_BANDIT_ALGORITHM_UNSPECIFIED {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("bandit_config.algorithm must not be UNSPECIFIED"))
	}
	if cfg.GetRewardMetricId() == "" {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("bandit_config.reward_metric_id is required"))
	}
	frac := cfg.GetMinExplorationFraction()
	if frac != 0 && (frac < 0 || frac > 1) {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("bandit_config.min_exploration_fraction must be in (0.0, 1.0] or 0 (use default), got %f", frac))
	}
	if isContextual && len(cfg.GetContextFeatureKeys()) == 0 {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("bandit_config.context_feature_keys must have >= 1 entry for CONTEXTUAL_BANDIT"))
	}
	return nil
}

func validateSessionConfig(exp *commonv1.Experiment) *connect.Error {
	cfg := exp.GetSessionConfig()
	if cfg == nil {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("session_config is required for SESSION_LEVEL type"))
	}
	if cfg.GetSessionIdAttribute() == "" {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("session_config.session_id_attribute is required"))
	}
	if cfg.GetMinSessionsPerUser() < 0 {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("session_config.min_sessions_per_user must be >= 0, got %d", cfg.GetMinSessionsPerUser()))
	}
	return nil
}
