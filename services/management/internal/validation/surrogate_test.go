package validation

import (
	"testing"

	"connectrpc.com/connect"
	"github.com/stretchr/testify/assert"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

func validSurrogateModel() *commonv1.SurrogateModelConfig {
	return &commonv1.SurrogateModelConfig{
		TargetMetricId:        "90_day_churn_rate",
		InputMetricIds:        []string{"7d_watch_time", "7d_session_freq"},
		ModelType:             commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_LINEAR,
		ObservationWindowDays: 7,
		PredictionHorizonDays: 90,
	}
}

func TestValidateCreateSurrogateModel(t *testing.T) {
	t.Run("valid linear model", func(t *testing.T) {
		err := ValidateCreateSurrogateModel(validSurrogateModel())
		assert.Nil(t, err)
	})

	t.Run("nil model", func(t *testing.T) {
		err := ValidateCreateSurrogateModel(nil)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
	})

	t.Run("missing target_metric_id", func(t *testing.T) {
		m := validSurrogateModel()
		m.TargetMetricId = ""
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "target_metric_id")
	})

	t.Run("empty input_metric_ids", func(t *testing.T) {
		m := validSurrogateModel()
		m.InputMetricIds = nil
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "input_metric_ids")
	})

	t.Run("input_metric_ids with empty string", func(t *testing.T) {
		m := validSurrogateModel()
		m.InputMetricIds = []string{"7d_watch_time", ""}
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "input_metric_ids[1]")
	})

	t.Run("unspecified model_type", func(t *testing.T) {
		m := validSurrogateModel()
		m.ModelType = commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_UNSPECIFIED
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "model_type")
	})

	t.Run("observation_window_days <= 0", func(t *testing.T) {
		m := validSurrogateModel()
		m.ObservationWindowDays = 0
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "observation_window_days")
	})

	t.Run("prediction_horizon_days <= 0", func(t *testing.T) {
		m := validSurrogateModel()
		m.PredictionHorizonDays = 0
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "prediction_horizon_days")
	})

	t.Run("prediction_horizon <= observation_window", func(t *testing.T) {
		m := validSurrogateModel()
		m.ObservationWindowDays = 30
		m.PredictionHorizonDays = 30
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "prediction_horizon_days")
	})

	t.Run("prediction_horizon < observation_window", func(t *testing.T) {
		m := validSurrogateModel()
		m.ObservationWindowDays = 30
		m.PredictionHorizonDays = 14
		err := ValidateCreateSurrogateModel(m)
		assert.NotNil(t, err)
		assert.Equal(t, connect.CodeInvalidArgument, err.Code())
		assert.Contains(t, err.Error(), "prediction_horizon_days")
	})
}
