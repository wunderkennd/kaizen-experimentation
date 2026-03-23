package validation

import (
	"fmt"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// ValidateCreateMetricDefinition validates a metric definition for creation.
func ValidateCreateMetricDefinition(m *commonv1.MetricDefinition) *connect.Error {
	if m == nil {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("metric is required"))
	}
	if m.GetName() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("name is required"))
	}
	if m.GetType() == commonv1.MetricType_METRIC_TYPE_UNSPECIFIED {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("type is required"))
	}

	if err := validateMetricTypeFields(m); err != nil {
		return err
	}
	return validateMetricStakeholderAggregation(m)
}

// validateMetricStakeholderAggregation enforces ADR-014 structural rules at
// metric definition time:
//   - MetricStakeholder must be set (not UNSPECIFIED).
//   - MetricAggregationLevel must be set (not UNSPECIFIED).
//   - PROVIDER aggregation is only valid for PROVIDER-stakeholder metrics.
//
// Use-case constraints (bandit rewards must use USER aggregation; guardrails
// accept USER or EXPERIMENT) are enforced at experiment-start time via
// ValidateBanditRewardMetricAggregation and ValidateGuardrailMetricAggregation
// when the actual metric definition is fetched from the store.
func validateMetricStakeholderAggregation(m *commonv1.MetricDefinition) *connect.Error {
	stakeholder := m.GetStakeholder()
	aggLevel := m.GetAggregationLevel()

	if stakeholder == commonv1.MetricStakeholder_METRIC_STAKEHOLDER_UNSPECIFIED {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("stakeholder is required (ADR-014): set USER, PROVIDER, or PLATFORM"))
	}
	if aggLevel == commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_UNSPECIFIED {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("aggregation_level is required (ADR-014): set USER, EXPERIMENT, or PROVIDER"))
	}

	// PROVIDER aggregation is only meaningful for provider-stakeholder metrics.
	if aggLevel == commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_PROVIDER &&
		stakeholder != commonv1.MetricStakeholder_METRIC_STAKEHOLDER_PROVIDER {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("aggregation_level PROVIDER is only valid for PROVIDER stakeholder metrics; got stakeholder %s", stakeholder))
	}

	return nil
}

// ValidateBanditRewardMetricAggregation enforces that a bandit reward metric
// uses USER aggregation (ADR-014). Called at experiment-start time after
// fetching the metric definition from the store.
func ValidateBanditRewardMetricAggregation(m *commonv1.MetricDefinition) *connect.Error {
	if m.GetAggregationLevel() != commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("bandit reward metric %q must use USER aggregation_level (ADR-014); got %s",
				m.GetName(), m.GetAggregationLevel()))
	}
	return nil
}

// ValidateGuardrailMetricAggregation enforces that a guardrail metric uses
// USER or EXPERIMENT aggregation (ADR-014). PROVIDER aggregation is not
// supported for guardrails. Called at experiment-start time.
func ValidateGuardrailMetricAggregation(m *commonv1.MetricDefinition) *connect.Error {
	level := m.GetAggregationLevel()
	switch level {
	case commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER,
		commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_EXPERIMENT:
		return nil
	default:
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("guardrail metric %q must use USER or EXPERIMENT aggregation_level (ADR-014); got %s",
				m.GetName(), level))
	}
}

func validateMetricTypeFields(m *commonv1.MetricDefinition) *connect.Error {
	switch m.GetType() {
	case commonv1.MetricType_METRIC_TYPE_MEAN,
		commonv1.MetricType_METRIC_TYPE_PROPORTION,
		commonv1.MetricType_METRIC_TYPE_COUNT:
		if m.GetSourceEventType() == "" {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("source_event_type is required for %s metrics", m.GetType()))
		}

	case commonv1.MetricType_METRIC_TYPE_RATIO:
		if m.GetNumeratorEventType() == "" {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("numerator_event_type is required for RATIO metrics"))
		}
		if m.GetDenominatorEventType() == "" {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("denominator_event_type is required for RATIO metrics"))
		}

	case commonv1.MetricType_METRIC_TYPE_PERCENTILE:
		if m.GetSourceEventType() == "" {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("source_event_type is required for PERCENTILE metrics"))
		}
		p := m.GetPercentile()
		if p <= 0 || p >= 1 {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("percentile must be in (0.0, 1.0), got %f", p))
		}

	case commonv1.MetricType_METRIC_TYPE_CUSTOM:
		if m.GetCustomSql() == "" {
			return connect.NewError(connect.CodeInvalidArgument,
				fmt.Errorf("custom_sql is required for CUSTOM metrics"))
		}
	}

	if mde := m.GetMinimumDetectableEffect(); mde < 0 {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("minimum_detectable_effect must be non-negative, got %f", mde))
	}

	return nil
}
