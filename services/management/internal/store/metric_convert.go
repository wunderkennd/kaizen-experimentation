package store

import (
	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

// --- MetricType conversions ---

var metricTypeToString = map[commonv1.MetricType]string{
	commonv1.MetricType_METRIC_TYPE_MEAN:       "MEAN",
	commonv1.MetricType_METRIC_TYPE_PROPORTION: "PROPORTION",
	commonv1.MetricType_METRIC_TYPE_RATIO:      "RATIO",
	commonv1.MetricType_METRIC_TYPE_COUNT:      "COUNT",
	commonv1.MetricType_METRIC_TYPE_PERCENTILE: "PERCENTILE",
	commonv1.MetricType_METRIC_TYPE_CUSTOM:     "CUSTOM",
}

var stringToMetricType = map[string]commonv1.MetricType{
	"MEAN":       commonv1.MetricType_METRIC_TYPE_MEAN,
	"PROPORTION": commonv1.MetricType_METRIC_TYPE_PROPORTION,
	"RATIO":      commonv1.MetricType_METRIC_TYPE_RATIO,
	"COUNT":      commonv1.MetricType_METRIC_TYPE_COUNT,
	"PERCENTILE": commonv1.MetricType_METRIC_TYPE_PERCENTILE,
	"CUSTOM":     commonv1.MetricType_METRIC_TYPE_CUSTOM,
}

// MetricTypeToString converts a proto MetricType to a DB string.
func MetricTypeToString(t commonv1.MetricType) string {
	return metricTypeToString[t]
}

// StringToMetricType converts a DB string to a proto MetricType.
func StringToMetricType(s string) commonv1.MetricType {
	return stringToMetricType[s]
}

// MetricDefinitionToRow converts a proto MetricDefinition to a DB row.
func MetricDefinitionToRow(m *commonv1.MetricDefinition) MetricDefinitionRow {
	row := MetricDefinitionRow{
		MetricID:               m.GetMetricId(),
		Name:                   m.GetName(),
		Description:            m.GetDescription(),
		Type:                   MetricTypeToString(m.GetType()),
		SourceEventType:        m.GetSourceEventType(),
		NumeratorEventType:     m.GetNumeratorEventType(),
		DenominatorEventType:   m.GetDenominatorEventType(),
		CustomSQL:              m.GetCustomSql(),
		LowerIsBetter:          m.GetLowerIsBetter(),
		IsQoeMetric:            m.GetIsQoeMetric(),
		CupedCovariateMetricID: m.GetCupedCovariateMetricId(),
	}

	if p := m.GetPercentile(); p != 0 {
		row.Percentile = &p
	}
	if mde := m.GetMinimumDetectableEffect(); mde != 0 {
		row.MinimumDetectableEffect = &mde
	}

	return row
}

// RowToMetricDefinition converts a DB row to a proto MetricDefinition.
func RowToMetricDefinition(row MetricDefinitionRow) *commonv1.MetricDefinition {
	m := &commonv1.MetricDefinition{
		MetricId:               row.MetricID,
		Name:                   row.Name,
		Description:            row.Description,
		Type:                   StringToMetricType(row.Type),
		SourceEventType:        row.SourceEventType,
		NumeratorEventType:     row.NumeratorEventType,
		DenominatorEventType:   row.DenominatorEventType,
		CustomSql:              row.CustomSQL,
		LowerIsBetter:          row.LowerIsBetter,
		IsQoeMetric:            row.IsQoeMetric,
		CupedCovariateMetricId: row.CupedCovariateMetricID,
	}

	if row.Percentile != nil {
		m.Percentile = *row.Percentile
	}
	if row.MinimumDetectableEffect != nil {
		m.MinimumDetectableEffect = *row.MinimumDetectableEffect
	}

	return m
}
