package store

import (
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// --- SurrogateModelType conversions ---

var surrogateModelTypeToString = map[commonv1.SurrogateModelType]string{
	commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_LINEAR:           "LINEAR",
	commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_GRADIENT_BOOSTED: "GRADIENT_BOOSTED",
	commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_NEURAL:           "NEURAL",
}

var stringToSurrogateModelType = map[string]commonv1.SurrogateModelType{
	"LINEAR":           commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_LINEAR,
	"GRADIENT_BOOSTED": commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_GRADIENT_BOOSTED,
	"NEURAL":           commonv1.SurrogateModelType_SURROGATE_MODEL_TYPE_NEURAL,
}

// SurrogateModelTypeToString converts a proto SurrogateModelType to a DB string.
func SurrogateModelTypeToString(t commonv1.SurrogateModelType) string {
	return surrogateModelTypeToString[t]
}

// StringToSurrogateModelType converts a DB string to a proto SurrogateModelType.
func StringToSurrogateModelType(s string) commonv1.SurrogateModelType {
	return stringToSurrogateModelType[s]
}

// SurrogateModelToRow converts a proto SurrogateModelConfig to a DB row.
func SurrogateModelToRow(m *commonv1.SurrogateModelConfig) SurrogateModelRow {
	row := SurrogateModelRow{
		ModelID:               m.GetModelId(),
		TargetMetricID:        m.GetTargetMetricId(),
		InputMetricIDs:        m.GetInputMetricIds(),
		ObservationWindowDays: m.GetObservationWindowDays(),
		PredictionHorizonDays: m.GetPredictionHorizonDays(),
		ModelType:             SurrogateModelTypeToString(m.GetModelType()),
		MlflowModelURI:       m.GetMlflowModelUri(),
	}

	if r := m.GetCalibrationRSquared(); r != 0 {
		row.CalibrationRSquared = &r
	}

	return row
}

// RowToSurrogateModel converts a DB row to a proto SurrogateModelConfig.
func RowToSurrogateModel(row SurrogateModelRow) *commonv1.SurrogateModelConfig {
	m := &commonv1.SurrogateModelConfig{
		ModelId:               row.ModelID,
		TargetMetricId:        row.TargetMetricID,
		InputMetricIds:        row.InputMetricIDs,
		ObservationWindowDays: row.ObservationWindowDays,
		PredictionHorizonDays: row.PredictionHorizonDays,
		ModelType:             StringToSurrogateModelType(row.ModelType),
		MlflowModelUri:       row.MlflowModelURI,
	}

	if row.CalibrationRSquared != nil {
		m.CalibrationRSquared = *row.CalibrationRSquared
	}
	if row.LastCalibratedAt != nil {
		m.LastCalibratedAt = timestamppb.New(*row.LastCalibratedAt)
	}
	m.CreatedAt = timestamppb.New(row.CreatedAt)

	return m
}
