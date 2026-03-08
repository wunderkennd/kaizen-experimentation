package validation

import (
	"fmt"

	"connectrpc.com/connect"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// ValidateCreateSurrogateModel validates a surrogate model config for creation.
func ValidateCreateSurrogateModel(m *commonv1.SurrogateModelConfig) *connect.Error {
	if m == nil {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("model is required"))
	}
	if m.GetTargetMetricId() == "" {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("target_metric_id is required"))
	}
	if len(m.GetInputMetricIds()) == 0 {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("input_metric_ids must have at least one entry"))
	}
	for i, id := range m.GetInputMetricIds() {
		if id == "" {
			return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("input_metric_ids[%d] must not be empty", i))
		}
	}
	if m.GetModelType() == commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_UNSPECIFIED {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("model_type is required"))
	}
	if m.GetObservationWindowDays() <= 0 {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("observation_window_days must be > 0"))
	}
	if m.GetPredictionHorizonDays() <= 0 {
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("prediction_horizon_days must be > 0"))
	}
	if m.GetPredictionHorizonDays() <= m.GetObservationWindowDays() {
		return connect.NewError(connect.CodeInvalidArgument,
			fmt.Errorf("prediction_horizon_days (%d) must be > observation_window_days (%d)",
				m.GetPredictionHorizonDays(), m.GetObservationWindowDays()))
	}
	return nil
}
