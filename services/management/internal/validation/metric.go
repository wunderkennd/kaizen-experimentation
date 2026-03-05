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

	return validateMetricTypeFields(m)
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
