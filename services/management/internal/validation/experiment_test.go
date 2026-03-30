package validation

import (
	"testing"
	"time"

	"connectrpc.com/connect"
	"google.golang.org/protobuf/types/known/durationpb"

	commonv1 "github.com/org/experimentation/gen/go/experimentation/common/v1"
)

func validABExperiment() *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:            "test-experiment",
		OwnerEmail:      "test@example.com",
		LayerId:         "a0000000-0000-0000-0000-000000000001",
		PrimaryMetricId: "watch_time_minutes",
		Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_AB,
		Variants: []*commonv1.Variant{
			{Name: "control", TrafficFraction: 0.5, IsControl: true},
			{Name: "treatment", TrafficFraction: 0.5, IsControl: false},
		},
	}
}

func TestValidateCreateExperiment(t *testing.T) {
	tests := []struct {
		name     string
		modify   func(e *commonv1.Experiment)
		wantCode connect.Code
		wantOK   bool
	}{
		{
			name:   "valid AB experiment",
			modify: func(e *commonv1.Experiment) {},
			wantOK: true,
		},
		{
			name:     "nil experiment",
			modify:   nil, // will pass nil
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name:     "missing name",
			modify:   func(e *commonv1.Experiment) { e.Name = "" },
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name:     "missing owner_email",
			modify:   func(e *commonv1.Experiment) { e.OwnerEmail = "" },
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name:     "missing layer_id",
			modify:   func(e *commonv1.Experiment) { e.LayerId = "" },
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name:     "missing primary_metric_id",
			modify:   func(e *commonv1.Experiment) { e.PrimaryMetricId = "" },
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name:     "unspecified type",
			modify:   func(e *commonv1.Experiment) { e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_UNSPECIFIED },
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name:     "only 1 variant for AB",
			modify:   func(e *commonv1.Experiment) { e.Variants = e.Variants[:1] },
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "no control variant",
			modify: func(e *commonv1.Experiment) {
				e.Variants[0].IsControl = false
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "two control variants",
			modify: func(e *commonv1.Experiment) {
				e.Variants[1].IsControl = true
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "fractions don't sum to 1.0",
			modify: func(e *commonv1.Experiment) {
				e.Variants[0].TrafficFraction = 0.3
				e.Variants[1].TrafficFraction = 0.3
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "fraction out of range",
			modify: func(e *commonv1.Experiment) {
				e.Variants[0].TrafficFraction = 1.5
				e.Variants[1].TrafficFraction = -0.5
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "variant missing name",
			modify: func(e *commonv1.Experiment) {
				e.Variants[0].Name = ""
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "interleaving without config",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "interleaving with config - valid",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING
				e.InterleavingConfig = &commonv1.InterleavingConfig{
					Method:       commonv1.InterleavingMethod_INTERLEAVING_METHOD_TEAM_DRAFT,
					AlgorithmIds: []string{"algo-a", "algo-b"},
				}
			},
			wantOK: true,
		},
		{
			name: "interleaving unspecified method",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING
				e.InterleavingConfig = &commonv1.InterleavingConfig{
					AlgorithmIds: []string{"algo-a", "algo-b"},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "interleaving too few algorithm_ids",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING
				e.InterleavingConfig = &commonv1.InterleavingConfig{
					Method:       commonv1.InterleavingMethod_INTERLEAVING_METHOD_TEAM_DRAFT,
					AlgorithmIds: []string{"algo-a"},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "interleaving multileave needs 3 algorithms",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING
				e.InterleavingConfig = &commonv1.InterleavingConfig{
					Method:       commonv1.InterleavingMethod_INTERLEAVING_METHOD_MULTILEAVE,
					AlgorithmIds: []string{"algo-a", "algo-b"},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "interleaving empty algorithm_id",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_INTERLEAVING
				e.InterleavingConfig = &commonv1.InterleavingConfig{
					Method:       commonv1.InterleavingMethod_INTERLEAVING_METHOD_TEAM_DRAFT,
					AlgorithmIds: []string{"algo-a", ""},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "MAB without bandit_config",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_MAB
				e.Variants[0].IsControl = false
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "MAB unspecified algorithm",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_MAB
				e.Variants[0].IsControl = false
				e.BanditConfig = &commonv1.BanditConfig{
					RewardMetricId: "watch_time_minutes",
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "MAB missing reward_metric_id",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_MAB
				e.Variants[0].IsControl = false
				e.BanditConfig = &commonv1.BanditConfig{
					Algorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "MAB exploration_fraction out of range",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_MAB
				e.Variants[0].IsControl = false
				e.BanditConfig = &commonv1.BanditConfig{
					Algorithm:              commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					RewardMetricId:         "watch_time_minutes",
					MinExplorationFraction: 1.5,
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "MAB valid thompson_sampling",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_MAB
				e.Variants[0].IsControl = false
				e.BanditConfig = &commonv1.BanditConfig{
					Algorithm:      commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					RewardMetricId: "watch_time_minutes",
				}
			},
			wantOK: true,
		},
		{
			name: "contextual_bandit missing context_features",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT
				e.Variants[0].IsControl = false
				e.BanditConfig = &commonv1.BanditConfig{
					Algorithm:      commonv1.BanditAlgorithm_BANDIT_ALGORITHM_LINEAR_UCB,
					RewardMetricId: "watch_time_minutes",
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "contextual_bandit valid",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_CONTEXTUAL_BANDIT
				e.Variants[0].IsControl = false
				e.BanditConfig = &commonv1.BanditConfig{
					Algorithm:          commonv1.BanditAlgorithm_BANDIT_ALGORITHM_LINEAR_UCB,
					RewardMetricId:     "watch_time_minutes",
					ContextFeatureKeys: []string{"device_type"},
				}
			},
			wantOK: true,
		},
		{
			name: "session missing session_id_attribute",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL
				e.SessionConfig = &commonv1.SessionConfig{}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "session valid",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL
				e.SessionConfig = &commonv1.SessionConfig{
					SessionIdAttribute: "session_id",
				}
			},
			wantOK: true,
		},
		{
			name: "session_level without session_config",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_SESSION_LEVEL
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "cumulative_holdout without flag",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "cumulative_holdout valid",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT
				e.IsCumulativeHoldout = true
			},
			wantOK: true,
		},
		{
			name: "cumulative_holdout with AUTO_PAUSE guardrail_action",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT
				e.IsCumulativeHoldout = true
				e.GuardrailAction = commonv1.GuardrailAction_GUARDRAIL_ACTION_AUTO_PAUSE
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "cumulative_holdout with ALERT_ONLY guardrail_action",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_CUMULATIVE_HOLDOUT
				e.IsCumulativeHoldout = true
				e.GuardrailAction = commonv1.GuardrailAction_GUARDRAIL_ACTION_ALERT_ONLY
			},
			wantOK: true,
		},

		// META experiment type tests (ADR-013).
		{
			name: "META without meta_experiment_config",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META with unspecified base_algorithm",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					VariantObjectives: []*commonv1.MetaVariantObjective{
						{VariantId: "control", RewardWeights: map[string]float64{"watch_time": 1.0}},
					},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META with empty variant_objectives",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					BaseAlgorithm:     commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					VariantObjectives: []*commonv1.MetaVariantObjective{},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META with variant_id not in variants",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.Variants[0].VariantId = "control"
				e.Variants[1].VariantId = "treatment"
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					BaseAlgorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					VariantObjectives: []*commonv1.MetaVariantObjective{
						{VariantId: "control", RewardWeights: map[string]float64{"watch_time": 1.0}},
						{VariantId: "nonexistent", RewardWeights: map[string]float64{"watch_time": 1.0}},
					},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META with empty variant_id in objective",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.Variants[0].VariantId = "control"
				e.Variants[1].VariantId = "treatment"
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					BaseAlgorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					VariantObjectives: []*commonv1.MetaVariantObjective{
						{VariantId: "", RewardWeights: map[string]float64{"watch_time": 1.0}},
					},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META with reward_weights not summing to 1.0",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.Variants[0].VariantId = "control"
				e.Variants[1].VariantId = "treatment"
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					BaseAlgorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					VariantObjectives: []*commonv1.MetaVariantObjective{
						{VariantId: "control", RewardWeights: map[string]float64{"watch_time": 0.3, "engagement": 0.3}},
					},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META with empty reward_weights",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.Variants[0].VariantId = "control"
				e.Variants[1].VariantId = "treatment"
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					BaseAlgorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					VariantObjectives: []*commonv1.MetaVariantObjective{
						{VariantId: "control", RewardWeights: map[string]float64{}},
					},
				}
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META valid — two variants with distinct reward objectives",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.Variants[0].VariantId = "obj_watch_time"
				e.Variants[0].IsControl = false
				e.Variants[1].VariantId = "obj_engagement"
				e.Variants[1].IsControl = false
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					BaseAlgorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					VariantObjectives: []*commonv1.MetaVariantObjective{
						{
							VariantId:     "obj_watch_time",
							RewardWeights: map[string]float64{"watch_time": 1.0},
						},
						{
							VariantId:     "obj_engagement",
							RewardWeights: map[string]float64{"engagement": 0.6, "watch_time": 0.4},
						},
					},
					OutcomeMetricIds: []string{"watch_time", "engagement", "retention_d30"},
				}
				e.BanditConfig = &commonv1.BanditConfig{
					Algorithm:      commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					RewardMetricId: "watch_time",
				}
			},
			wantOK: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var exp *commonv1.Experiment
			if tt.modify != nil {
				exp = validABExperiment()
				tt.modify(exp)
			}

			err := ValidateCreateExperiment(exp)
			if tt.wantOK {
				if err != nil {
					t.Errorf("expected no error, got: %v", err)
				}
				return
			}

			if err == nil {
				t.Fatal("expected error, got nil")
			}
			if err.Code() != tt.wantCode {
				t.Errorf("expected code %v, got %v: %v", tt.wantCode, err.Code(), err.Message())
			}
		})
	}
}

func TestValidateCreateExperiment_Phase5Types(t *testing.T) {
	tests := []struct {
		name     string
		modify   func(e *commonv1.Experiment)
		wantCode connect.Code
		wantOK   bool
	}{
		// META
		{
			name: "META without meta_experiment_config",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "META with meta_experiment_config - valid at create",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_META
				e.Variants = []*commonv1.Variant{
					{VariantId: "v-ctrl", Name: "control", TrafficFraction: 0.5, IsControl: false},
					{VariantId: "v-treat", Name: "treatment", TrafficFraction: 0.5, IsControl: false},
				}
				e.MetaExperimentConfig = &commonv1.MetaExperimentConfig{
					BaseAlgorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
					VariantObjectives: []*commonv1.MetaVariantObjective{
						{VariantId: "v-ctrl", RewardWeights: map[string]float64{"watch_time": 1.0}},
						{VariantId: "v-treat", RewardWeights: map[string]float64{"watch_time": 1.0}},
					},
				}
			},
			wantOK: true,
		},
		// SWITCHBACK
		{
			name: "SWITCHBACK without switchback_config",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_SWITCHBACK
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "SWITCHBACK with switchback_config - valid at create",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_SWITCHBACK
				e.SwitchbackConfig = &commonv1.SwitchbackConfig{
					PlannedCycles: 4,
					BlockDuration: durationpb.New(time.Hour),
				}
			},
			wantOK: true,
		},
		// QUASI
		{
			name: "QUASI without quasi_experiment_config",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_QUASI
			},
			wantCode: connect.CodeInvalidArgument,
		},
		{
			name: "QUASI with quasi_experiment_config - valid at create",
			modify: func(e *commonv1.Experiment) {
				e.Type = commonv1.ExperimentType_EXPERIMENT_TYPE_QUASI
				e.QuasiExperimentConfig = &commonv1.QuasiExperimentConfig{
					TreatedUnitId: "market-us",
					DonorUnitIds:  []string{"market-uk"},
				}
			},
			wantOK: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			exp := validABExperiment()
			tt.modify(exp)

			err := ValidateCreateExperiment(exp)
			if tt.wantOK {
				if err != nil {
					t.Errorf("expected no error, got: %v", err)
				}
				return
			}
			if err == nil {
				t.Fatal("expected error, got nil")
			}
			if err.Code() != tt.wantCode {
				t.Errorf("expected code %v, got %v: %v", tt.wantCode, err.Code(), err.Message())
			}
		})
	}
}

func validMetaExperiment() *commonv1.Experiment {
	return &commonv1.Experiment{
		Name:            "meta-experiment",
		OwnerEmail:      "test@example.com",
		LayerId:         "a0000000-0000-0000-0000-000000000001",
		PrimaryMetricId: "watch_time_minutes",
		Type:            commonv1.ExperimentType_EXPERIMENT_TYPE_META,
		Variants: []*commonv1.Variant{
			{VariantId: "v-control", Name: "control", TrafficFraction: 0.5, IsControl: true},
			{VariantId: "v-treatment", Name: "treatment", TrafficFraction: 0.5},
		},
		MetaExperimentConfig: &commonv1.MetaExperimentConfig{
			BaseAlgorithm: commonv1.BanditAlgorithm_BANDIT_ALGORITHM_THOMPSON_SAMPLING,
			VariantObjectives: []*commonv1.MetaVariantObjective{
				{VariantId: "v-control", RewardWeights: map[string]float64{"watch_time_minutes": 1.0}},
				{VariantId: "v-treatment", RewardWeights: map[string]float64{"watch_time_minutes": 0.7, "completion_rate": 0.3}},
			},
		},
	}
}

func TestValidateMetaExperimentForStart(t *testing.T) {
	t.Run("nil meta_experiment_config", func(t *testing.T) {
		exp := validMetaExperiment()
		exp.MetaExperimentConfig = nil
		err := ValidateMetaExperimentForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument, got %v", err)
		}
	})

	t.Run("empty variant_objectives", func(t *testing.T) {
		exp := validMetaExperiment()
		exp.MetaExperimentConfig.VariantObjectives = nil
		err := ValidateMetaExperimentForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument for empty objectives, got %v", err)
		}
	})

	t.Run("variant_id not in experiment variants", func(t *testing.T) {
		exp := validMetaExperiment()
		exp.MetaExperimentConfig.VariantObjectives[0].VariantId = "v-unknown"
		err := ValidateMetaExperimentForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument for unknown variant_id, got %v", err)
		}
	})

	t.Run("empty variant_id in objective", func(t *testing.T) {
		exp := validMetaExperiment()
		exp.MetaExperimentConfig.VariantObjectives[0].VariantId = ""
		err := ValidateMetaExperimentForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument for empty variant_id, got %v", err)
		}
	})

	t.Run("valid meta experiment", func(t *testing.T) {
		exp := validMetaExperiment()
		err := ValidateMetaExperimentForStart(exp)
		if err != nil {
			t.Errorf("expected no error, got: %v", err)
		}
	})
}

func TestValidateSwitchbackForStart(t *testing.T) {
	validSwitchback := func() *commonv1.Experiment {
		return &commonv1.Experiment{
			Type: commonv1.ExperimentType_EXPERIMENT_TYPE_SWITCHBACK,
			SwitchbackConfig: &commonv1.SwitchbackConfig{
				PlannedCycles: 4,
				BlockDuration: durationpb.New(time.Hour),
			},
		}
	}

	t.Run("nil switchback_config", func(t *testing.T) {
		exp := validSwitchback()
		exp.SwitchbackConfig = nil
		err := ValidateSwitchbackForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument, got %v", err)
		}
	})

	t.Run("planned_cycles too low", func(t *testing.T) {
		exp := validSwitchback()
		exp.SwitchbackConfig.PlannedCycles = 3
		err := ValidateSwitchbackForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument for planned_cycles=3, got %v", err)
		}
	})

	t.Run("block_duration nil", func(t *testing.T) {
		exp := validSwitchback()
		exp.SwitchbackConfig.BlockDuration = nil
		err := ValidateSwitchbackForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument for nil block_duration, got %v", err)
		}
	})

	t.Run("block_duration too short", func(t *testing.T) {
		exp := validSwitchback()
		exp.SwitchbackConfig.BlockDuration = durationpb.New(30 * time.Minute)
		err := ValidateSwitchbackForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument for 30m block_duration, got %v", err)
		}
	})

	t.Run("valid: planned_cycles=4 block_duration=1h", func(t *testing.T) {
		exp := validSwitchback()
		err := ValidateSwitchbackForStart(exp)
		if err != nil {
			t.Errorf("expected no error, got: %v", err)
		}
	})

	t.Run("valid: planned_cycles=8 block_duration=2h", func(t *testing.T) {
		exp := validSwitchback()
		exp.SwitchbackConfig.PlannedCycles = 8
		exp.SwitchbackConfig.BlockDuration = durationpb.New(2 * time.Hour)
		err := ValidateSwitchbackForStart(exp)
		if err != nil {
			t.Errorf("expected no error, got: %v", err)
		}
	})
}

func TestValidateQuasiExperimentForStart(t *testing.T) {
	validQuasi := func() *commonv1.Experiment {
		return &commonv1.Experiment{
			Type: commonv1.ExperimentType_EXPERIMENT_TYPE_QUASI,
			QuasiExperimentConfig: &commonv1.QuasiExperimentConfig{
				TreatedUnitId: "market-us",
				DonorUnitIds:  []string{"market-uk", "market-de"},
			},
		}
	}

	t.Run("nil quasi_experiment_config", func(t *testing.T) {
		exp := validQuasi()
		exp.QuasiExperimentConfig = nil
		err := ValidateQuasiExperimentForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument, got %v", err)
		}
	})

	t.Run("empty donor_unit_ids", func(t *testing.T) {
		exp := validQuasi()
		exp.QuasiExperimentConfig.DonorUnitIds = nil
		err := ValidateQuasiExperimentForStart(exp)
		if err == nil || err.Code() != connect.CodeInvalidArgument {
			t.Errorf("expected InvalidArgument for empty donor_unit_ids, got %v", err)
		}
	})

	t.Run("valid quasi experiment", func(t *testing.T) {
		exp := validQuasi()
		err := ValidateQuasiExperimentForStart(exp)
		if err != nil {
			t.Errorf("expected no error, got: %v", err)
		}
	})
}

func TestValidateUpdateExperiment(t *testing.T) {
	t.Run("missing experiment_id", func(t *testing.T) {
		exp := validABExperiment()
		err := ValidateUpdateExperiment(exp)
		if err == nil {
			t.Fatal("expected error for missing experiment_id")
		}
	})

	t.Run("valid update", func(t *testing.T) {
		exp := validABExperiment()
		exp.ExperimentId = "e0000000-0000-0000-0000-000000000001"
		err := ValidateUpdateExperiment(exp)
		if err != nil {
			t.Errorf("unexpected error: %v", err)
		}
	})
}
