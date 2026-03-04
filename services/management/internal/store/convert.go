package store

import (
	"encoding/json"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// --- State conversions ---

var stateToString = map[commonv1.ExperimentState]string{
	commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT:      "DRAFT",
	commonv1.ExperimentState_EXPERIMENT_STATE_STARTING:   "STARTING",
	commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING:    "RUNNING",
	commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDING: "CONCLUDING",
	commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED:  "CONCLUDED",
	commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED:   "ARCHIVED",
}

var stringToState = map[string]commonv1.ExperimentState{
	"DRAFT":      commonv1.ExperimentState_EXPERIMENT_STATE_DRAFT,
	"STARTING":   commonv1.ExperimentState_EXPERIMENT_STATE_STARTING,
	"RUNNING":    commonv1.ExperimentState_EXPERIMENT_STATE_RUNNING,
	"CONCLUDING": commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDING,
	"CONCLUDED":  commonv1.ExperimentState_EXPERIMENT_STATE_CONCLUDED,
	"ARCHIVED":   commonv1.ExperimentState_EXPERIMENT_STATE_ARCHIVED,
}

// --- Type conversions ---

var typeToString = map[commonv1.ExperimentType]string{
	commonv1.ExperimentType_EXPERIMENT_TYPE_AB:                "AB",
	commonv1.ExperimentType_EXPERIMENT_TYPE_MULTIVARIATE:      "MULTIVARIATE",
	commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING:      "INTERLEAVING",
	commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL:     "SESSION_LEVEL",
	commonv1.ExperimentType_EXPERIMENT_TYPE_PLAYBACK_QOE:      "PLAYBACK_QOE",
	commonv1.ExperimentType_EXPERIMENT_TYPE_MAB:               "MAB",
	commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT: "CONTEXTUAL_BANDIT",
	commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT: "CUMULATIVE_HOLDOUT",
}

var stringToType = map[string]commonv1.ExperimentType{
	"AB":                commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
	"MULTIVARIATE":      commonv1.ExperimentType_EXPERIMENT_TYPE_MULTIVARIATE,
	"INTERLEAVING":      commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING,
	"SESSION_LEVEL":     commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL,
	"PLAYBACK_QOE":      commonv1.ExperimentType_EXPERIMENT_TYPE_PLAYBACK_QOE,
	"MAB":               commonv1.ExperimentType_EXPERIMENT_TYPE_MAB,
	"CONTEXTUAL_BANDIT": commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT,
	"CUMULATIVE_HOLDOUT": commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT,
}

// --- Guardrail action conversions ---

var guardrailActionToString = map[commonv1.GuardrailAction]string{
	commonv1.GuardrailAction_GUARDRAIL_ACTION_AUTO_PAUSE:  "AUTO_PAUSE",
	commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY:  "ALERT_ONLY",
}

var stringToGuardrailAction = map[string]commonv1.GuardrailAction{
	"AUTO_PAUSE": commonv1.GuardrailAction_GUARDRAIL_ACTION_AUTO_PAUSE,
	"ALERT_ONLY": commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY,
}

// --- Sequential method conversions ---

var seqMethodToString = map[commonv1.SequentialMethod]string{
	commonv1.SequentialMethod_SEQUENTIAL_METHOD_MSPRT:      "MSPRT",
	commonv1.SequentialMethod_SEQUENTIAL_METHOD_GST_OBF:    "GST_OBF",
	commonv1.SequentialMethod_SEQUENTIAL_METHOD_GST_POCOCK: "GST_POCOCK",
}

var stringToSeqMethod = map[string]commonv1.SequentialMethod{
	"MSPRT":      commonv1.SequentialMethod_SEQUENTIAL_METHOD_MSPRT,
	"GST_OBF":    commonv1.SequentialMethod_SEQUENTIAL_METHOD_GST_OBF,
	"GST_POCOCK": commonv1.SequentialMethod_SEQUENTIAL_METHOD_GST_POCOCK,
}

// StateToString converts a proto ExperimentState to a DB string.
func StateToString(s commonv1.ExperimentState) string {
	return stateToString[s]
}

// StringToState converts a DB string to a proto ExperimentState.
func StringToState(s string) commonv1.ExperimentState {
	return stringToState[s]
}

// TypeToString converts a proto ExperimentType to a DB string.
func TypeToString(t commonv1.ExperimentType) string {
	return typeToString[t]
}

// StringToType converts a DB string to a proto ExperimentType.
func StringToType(s string) commonv1.ExperimentType {
	return stringToType[s]
}

// typeConfig bundles the type-specific config fields into a single JSONB blob.
type typeConfig struct {
	InterleavingConfig json.RawMessage `json:"interleaving_config,omitempty"`
	SessionConfig      json.RawMessage `json:"session_config,omitempty"`
	BanditConfig       json.RawMessage `json:"bandit_config,omitempty"`
	LifecycleConfig    json.RawMessage `json:"lifecycle_config,omitempty"`
}

// ExperimentToRow converts a proto Experiment to DB rows.
func ExperimentToRow(exp *commonv1.Experiment) (ExperimentRow, []VariantRow, []GuardrailConfigRow) {
	row := ExperimentRow{
		ExperimentID:        exp.GetExperimentId(),
		Name:                exp.GetName(),
		Description:         exp.GetDescription(),
		OwnerEmail:          exp.GetOwnerEmail(),
		Type:                TypeToString(exp.GetType()),
		State:               "DRAFT",
		LayerID:             exp.GetLayerId(),
		PrimaryMetricID:     exp.GetPrimaryMetricId(),
		SecondaryMetricIDs:  exp.GetSecondaryMetricIds(),
		GuardrailAction:     guardrailActionToString[exp.GetGuardrailAction()],
		IsCumulativeHoldout: exp.GetIsCumulativeHoldout(),
	}

	if row.GuardrailAction == "" {
		row.GuardrailAction = "AUTO_PAUSE"
	}
	if row.SecondaryMetricIDs == nil {
		row.SecondaryMetricIDs = []string{}
	}

	// Targeting rule
	if exp.GetTargetingRuleId() != "" {
		s := exp.GetTargetingRuleId()
		row.TargetingRuleID = &s
	}

	// Surrogate model
	if exp.GetSurrogateModelId() != "" {
		s := exp.GetSurrogateModelId()
		row.SurrogateModelID = &s
	}

	// Sequential test config
	if stc := exp.GetSequentialTestConfig(); stc != nil {
		if m := seqMethodToString[stc.GetMethod()]; m != "" {
			row.SequentialMethod = &m
		}
		if pl := stc.GetPlannedLooks(); pl > 0 {
			row.PlannedLooks = &pl
		}
		if a := stc.GetOverallAlpha(); a > 0 {
			row.OverallAlpha = &a
		}
	}

	// Type-specific config as JSONB
	tc := typeConfig{}
	if exp.GetInterleavingConfig() != nil {
		b, _ := json.Marshal(protoToMap(exp.GetInterleavingConfig()))
		tc.InterleavingConfig = b
	}
	if exp.GetSessionConfig() != nil {
		b, _ := json.Marshal(protoToMap(exp.GetSessionConfig()))
		tc.SessionConfig = b
	}
	if exp.GetBanditConfig() != nil {
		b, _ := json.Marshal(protoToMap(exp.GetBanditConfig()))
		tc.BanditConfig = b
	}
	if exp.GetLifecycleConfig() != nil {
		b, _ := json.Marshal(protoToMap(exp.GetLifecycleConfig()))
		tc.LifecycleConfig = b
	}
	row.TypeConfig, _ = json.Marshal(tc)

	// Variants
	variants := make([]VariantRow, 0, len(exp.GetVariants()))
	for i, v := range exp.GetVariants() {
		vr := VariantRow{
			ExperimentID:    exp.GetExperimentId(),
			Name:            v.GetName(),
			TrafficFraction: v.GetTrafficFraction(),
			IsControl:       v.GetIsControl(),
			Ordinal:         int32(i),
		}
		if v.GetPayloadJson() != "" {
			vr.PayloadJSON = json.RawMessage(v.GetPayloadJson())
		} else {
			vr.PayloadJSON = json.RawMessage(`{}`)
		}
		variants = append(variants, vr)
	}

	// Guardrails
	guardrails := make([]GuardrailConfigRow, 0, len(exp.GetGuardrailConfigs()))
	for _, g := range exp.GetGuardrailConfigs() {
		guardrails = append(guardrails, GuardrailConfigRow{
			ExperimentID:               exp.GetExperimentId(),
			MetricID:                   g.GetMetricId(),
			Threshold:                  g.GetThreshold(),
			ConsecutiveBreachesRequired: g.GetConsecutiveBreachesRequired(),
		})
	}

	return row, variants, guardrails
}

// RowToExperiment converts DB rows back to a proto Experiment.
func RowToExperiment(row ExperimentRow, variants []VariantRow, guardrails []GuardrailConfigRow) *commonv1.Experiment {
	exp := &commonv1.Experiment{
		ExperimentId:        row.ExperimentID,
		Name:                row.Name,
		Description:         row.Description,
		OwnerEmail:          row.OwnerEmail,
		Type:                StringToType(row.Type),
		State:               StringToState(row.State),
		LayerId:             row.LayerID,
		PrimaryMetricId:     row.PrimaryMetricID,
		SecondaryMetricIds:  row.SecondaryMetricIDs,
		GuardrailAction:     stringToGuardrailAction[row.GuardrailAction],
		IsCumulativeHoldout: row.IsCumulativeHoldout,
		HashSalt:            row.HashSalt,
		CreatedAt:           timestamppb.New(row.CreatedAt),
	}

	if row.TargetingRuleID != nil {
		exp.TargetingRuleId = *row.TargetingRuleID
	}
	if row.SurrogateModelID != nil {
		exp.SurrogateModelId = *row.SurrogateModelID
	}
	if row.StartedAt != nil {
		exp.StartedAt = timestamppb.New(*row.StartedAt)
	}
	if row.ConcludedAt != nil {
		exp.ConcludedAt = timestamppb.New(*row.ConcludedAt)
	}

	// Sequential test config
	if row.SequentialMethod != nil {
		stc := &commonv1.SequentialTestConfig{
			Method: stringToSeqMethod[*row.SequentialMethod],
		}
		if row.PlannedLooks != nil {
			stc.PlannedLooks = *row.PlannedLooks
		}
		if row.OverallAlpha != nil {
			stc.OverallAlpha = *row.OverallAlpha
		}
		exp.SequentialTestConfig = stc
	}

	// Type-specific config from JSONB
	if len(row.TypeConfig) > 0 {
		var tc typeConfig
		if err := json.Unmarshal(row.TypeConfig, &tc); err == nil {
			if len(tc.InterleavingConfig) > 0 {
				exp.InterleavingConfig = &commonv1.InterleavingConfig{}
				mapToProto(tc.InterleavingConfig, exp.InterleavingConfig)
			}
			if len(tc.SessionConfig) > 0 {
				exp.SessionConfig = &commonv1.SessionConfig{}
				mapToProto(tc.SessionConfig, exp.SessionConfig)
			}
			if len(tc.BanditConfig) > 0 {
				exp.BanditConfig = &commonv1.BanditConfig{}
				mapToProto(tc.BanditConfig, exp.BanditConfig)
			}
			if len(tc.LifecycleConfig) > 0 {
				exp.LifecycleConfig = &commonv1.LifecycleStratificationConfig{}
				mapToProto(tc.LifecycleConfig, exp.LifecycleConfig)
			}
		}
	}

	// Variants
	for _, v := range variants {
		pv := &commonv1.Variant{
			VariantId:       v.VariantID,
			Name:            v.Name,
			TrafficFraction: v.TrafficFraction,
			IsControl:       v.IsControl,
		}
		if len(v.PayloadJSON) > 0 && string(v.PayloadJSON) != "{}" {
			pv.PayloadJson = string(v.PayloadJSON)
		}
		exp.Variants = append(exp.Variants, pv)
	}

	// Guardrails
	for _, g := range guardrails {
		exp.GuardrailConfigs = append(exp.GuardrailConfigs, &commonv1.GuardrailConfig{
			MetricId:                   g.MetricID,
			Threshold:                  g.Threshold,
			ConsecutiveBreachesRequired: g.ConsecutiveBreachesRequired,
		})
	}

	return exp
}

// protoToMap serializes a proto message to a generic map for JSONB storage.
func protoToMap(msg interface{}) map[string]any {
	b, err := json.Marshal(msg)
	if err != nil {
		return nil
	}
	var m map[string]any
	json.Unmarshal(b, &m)
	return m
}

// mapToProto deserializes JSONB data into a proto message.
func mapToProto(data json.RawMessage, msg interface{}) {
	json.Unmarshal(data, msg)
}
