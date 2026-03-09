package validation

import (
	"testing"

	"connectrpc.com/connect"

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
