package validation

import (
	"testing"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"github.com/stretchr/testify/assert"
)

func TestValidateCreateMetricDefinition_RequiresName(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Type:            commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType: "page_view",
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "name is required")
}

func TestValidateCreateMetricDefinition_RequiresType(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "test_metric",
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "type is required")
}

func TestValidateCreateMetricDefinition_NilMetric(t *testing.T) {
	err := ValidateCreateMetricDefinition(nil)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "metric is required")
}

func TestValidateCreateMetricDefinition_MeanRequiresSourceEvent(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "test",
		Type: commonv1.MetricType_METRIC_TYPE_MEAN,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "source_event_type is required")
}

func TestValidateCreateMetricDefinition_ProportionRequiresSourceEvent(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "test",
		Type: commonv1.MetricType_METRIC_TYPE_PROPORTION,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "source_event_type is required")
}

func TestValidateCreateMetricDefinition_CountRequiresSourceEvent(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "test",
		Type: commonv1.MetricType_METRIC_TYPE_COUNT,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "source_event_type is required")
}

func TestValidateCreateMetricDefinition_RatioRequiresNumeratorDenominator(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "test",
		Type: commonv1.MetricType_METRIC_TYPE_RATIO,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "numerator_event_type is required")

	m.NumeratorEventType = "revenue"
	err = ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "denominator_event_type is required")
}

func TestValidateCreateMetricDefinition_PercentileRequiresSourceAndValue(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "test",
		Type: commonv1.MetricType_METRIC_TYPE_PERCENTILE,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "source_event_type is required")

	m.SourceEventType = "latency"
	err = ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "percentile must be in (0.0, 1.0)")

	m.Percentile = 1.5
	err = ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "percentile must be in (0.0, 1.0)")
}

func TestValidateCreateMetricDefinition_CustomRequiresSQL(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "test",
		Type: commonv1.MetricType_METRIC_TYPE_CUSTOM,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "custom_sql is required")
}

func TestValidateCreateMetricDefinition_NegativeMDE(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:                    "test",
		Type:                    commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType:         "view",
		MinimumDetectableEffect: -0.5,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "minimum_detectable_effect must be non-negative")
}

func TestValidateCreateMetricDefinition_ValidMean(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "avg_watch_time",
		Type:             commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType:  "watch_event",
		Stakeholder:      commonv1.MetricStakeholder_METRIC_STAKEHOLDER_USER,
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.Nil(t, err)
}

func TestValidateCreateMetricDefinition_ValidRatio(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:                 "revenue_per_session",
		Type:                 commonv1.MetricType_METRIC_TYPE_RATIO,
		NumeratorEventType:   "revenue",
		DenominatorEventType: "session",
		Stakeholder:          commonv1.MetricStakeholder_METRIC_STAKEHOLDER_PLATFORM,
		AggregationLevel:     commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.Nil(t, err)
}

func TestValidateCreateMetricDefinition_ValidPercentile(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "p95_ttff",
		Type:             commonv1.MetricType_METRIC_TYPE_PERCENTILE,
		SourceEventType:  "ttff_event",
		Percentile:       0.95,
		Stakeholder:      commonv1.MetricStakeholder_METRIC_STAKEHOLDER_USER,
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.Nil(t, err)
}

func TestValidateCreateMetricDefinition_ValidCustom(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "custom_engagement",
		Type:             commonv1.MetricType_METRIC_TYPE_CUSTOM,
		CustomSql:        "SELECT AVG(engagement_score) FROM events",
		Stakeholder:      commonv1.MetricStakeholder_METRIC_STAKEHOLDER_PLATFORM,
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_EXPERIMENT,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.Nil(t, err)
}

// --- ADR-014: MetricStakeholder / MetricAggregationLevel tests ---

func TestValidateCreateMetricDefinition_StakeholderRequired(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "some_metric",
		Type:             commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType:  "evt",
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER,
		// Stakeholder intentionally omitted (UNSPECIFIED)
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "stakeholder is required")
}

func TestValidateCreateMetricDefinition_AggregationLevelRequired(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:            "some_metric",
		Type:            commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType: "evt",
		Stakeholder:     commonv1.MetricStakeholder_METRIC_STAKEHOLDER_USER,
		// AggregationLevel intentionally omitted (UNSPECIFIED)
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "aggregation_level is required")
}

func TestValidateCreateMetricDefinition_ProviderAggRequiresProviderStakeholder(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "catalog_coverage",
		Type:             commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType:  "impression",
		Stakeholder:      commonv1.MetricStakeholder_METRIC_STAKEHOLDER_USER, // wrong for PROVIDER agg
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_PROVIDER,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "aggregation_level PROVIDER is only valid for PROVIDER stakeholder")
}

func TestValidateCreateMetricDefinition_ProviderAggWithProviderStakeholder(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "catalog_coverage",
		Type:             commonv1.MetricType_METRIC_TYPE_MEAN,
		SourceEventType:  "impression",
		Stakeholder:      commonv1.MetricStakeholder_METRIC_STAKEHOLDER_PROVIDER,
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_PROVIDER,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.Nil(t, err)
}

func TestValidateCreateMetricDefinition_UserMetricWithExperimentAgg(t *testing.T) {
	// USER stakeholder with EXPERIMENT aggregation is valid (for guardrail use).
	m := &commonv1.MetricDefinition{
		Name:             "churn_rate",
		Type:             commonv1.MetricType_METRIC_TYPE_PROPORTION,
		SourceEventType:  "churn",
		Stakeholder:      commonv1.MetricStakeholder_METRIC_STAKEHOLDER_USER,
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_EXPERIMENT,
	}
	err := ValidateCreateMetricDefinition(m)
	assert.Nil(t, err)
}

// --- ValidateBanditRewardMetricAggregation tests ---

func TestValidateBanditRewardMetricAggregation_UserAggValid(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "ctr",
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER,
	}
	assert.Nil(t, ValidateBanditRewardMetricAggregation(m))
}

func TestValidateBanditRewardMetricAggregation_ExperimentAggRejected(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "overall_conversion",
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_EXPERIMENT,
	}
	err := ValidateBanditRewardMetricAggregation(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "USER aggregation_level")
}

func TestValidateBanditRewardMetricAggregation_ProviderAggRejected(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "catalog_coverage",
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_PROVIDER,
	}
	err := ValidateBanditRewardMetricAggregation(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "USER aggregation_level")
}

// --- ValidateGuardrailMetricAggregation tests ---

func TestValidateGuardrailMetricAggregation_UserAggValid(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "error_rate",
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_USER,
	}
	assert.Nil(t, ValidateGuardrailMetricAggregation(m))
}

func TestValidateGuardrailMetricAggregation_ExperimentAggValid(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "churn_rate",
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_EXPERIMENT,
	}
	assert.Nil(t, ValidateGuardrailMetricAggregation(m))
}

func TestValidateGuardrailMetricAggregation_ProviderAggRejected(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name:             "catalog_metric",
		AggregationLevel: commonv1.MetricAggregationLevel_METRIC_AGGREGATION_LEVEL_PROVIDER,
	}
	err := ValidateGuardrailMetricAggregation(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "USER or EXPERIMENT aggregation_level")
}

func TestValidateGuardrailMetricAggregation_UnspecifiedRejected(t *testing.T) {
	m := &commonv1.MetricDefinition{
		Name: "some_guardrail",
		// AggregationLevel UNSPECIFIED
	}
	err := ValidateGuardrailMetricAggregation(m)
	assert.NotNil(t, err)
	assert.Contains(t, err.Error(), "USER or EXPERIMENT aggregation_level")
}
