package handlers

import (
	"context"
	"fmt"
	"sync"
	"testing"

	"connectrpc.com/connect"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// TestEvaluateFlag_ConcurrentRace verifies that 100 goroutines can evaluate
// the same flag simultaneously without data races. Run with -race.
func TestEvaluateFlag_ConcurrentRace(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "concurrent-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)
	flagID := created.Msg.GetFlagId()

	const goroutines = 100
	var wg sync.WaitGroup
	wg.Add(goroutines)

	errs := make(chan error, goroutines)
	values := make(chan string, goroutines)

	for i := 0; i < goroutines; i++ {
		go func(i int) {
			defer wg.Done()
			resp, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
				FlagId: flagID,
				UserId: fmt.Sprintf("user_%d", i),
			}))
			if err != nil {
				errs <- err
				return
			}
			values <- resp.Msg.GetValue()
		}(i)
	}

	wg.Wait()
	close(errs)
	close(values)

	for err := range errs {
		t.Errorf("concurrent evaluation error: %v", err)
	}

	for v := range values {
		assert.Contains(t, []string{"true", "false"}, v)
	}
}

// TestEvaluateFlag_AllFlagTypes verifies that disabled flags of every type
// (BOOLEAN, STRING, NUMERIC, JSON) return their default value.
func TestEvaluateFlag_AllFlagTypes(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	tests := []struct {
		name         string
		flagType     flagsv1.FlagType
		defaultValue string
	}{
		{"boolean-disabled", flagsv1.FlagType_FLAG_TYPE_BOOLEAN, "false"},
		{"string-disabled", flagsv1.FlagType_FLAG_TYPE_STRING, "default-string"},
		{"numeric-disabled", flagsv1.FlagType_FLAG_TYPE_NUMERIC, "42"},
		{"json-disabled", flagsv1.FlagType_FLAG_TYPE_JSON, `{"key":"value"}`},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
				Flag: &flagsv1.Flag{
					Name:              tt.name,
					Type:              tt.flagType,
					DefaultValue:      tt.defaultValue,
					Enabled:           false,
					RolloutPercentage: 1.0,
				},
			}))
			require.NoError(t, err)

			eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
				FlagId: created.Msg.GetFlagId(),
				UserId: "user_123",
			}))
			require.NoError(t, err)
			assert.Equal(t, tt.defaultValue, eval.Msg.GetValue(),
				"disabled %s flag must return default value", tt.flagType)
		})
	}
}

// TestEvaluateFlag_StringFullRollout verifies that a STRING flag at 100% rollout
// with no variants returns DefaultValue (not "true"). Only BOOLEAN flags
// synthesize "true" for in-rollout users without variants (evaluate.go:80-84).
func TestEvaluateFlag_StringFullRollout(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "string-full-rollout",
			Type:              flagsv1.FlagType_FLAG_TYPE_STRING,
			DefaultValue:      "hello-world",
			Enabled:           true,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)

	eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "user_123",
	}))
	require.NoError(t, err)
	// Non-BOOLEAN flags with no variants return DefaultValue even when "in rollout".
	// There is no opposite value to synthesize (unlike BOOLEAN's "true"/"false").
	assert.Equal(t, "hello-world", eval.Msg.GetValue())
}

// TestEvaluateFlag_UnicodeUserID verifies that user IDs containing Unicode
// characters (French, Chinese, emoji, Cyrillic) hash deterministically.
func TestEvaluateFlag_UnicodeUserID(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "unicode-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)
	flagID := created.Msg.GetFlagId()

	unicodeUsers := []string{
		"utilisateur_café",     // French with accent
		"用户_12345",           // Chinese characters
		"🎲🧪_user",           // Emoji
		"пользователь_тест",   // Cyrillic
	}

	for _, userID := range unicodeUsers {
		t.Run(userID, func(t *testing.T) {
			// Evaluate twice to confirm determinism.
			eval1, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
				FlagId: flagID,
				UserId: userID,
			}))
			require.NoError(t, err)

			eval2, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
				FlagId: flagID,
				UserId: userID,
			}))
			require.NoError(t, err)

			assert.Equal(t, eval1.Msg.GetValue(), eval2.Msg.GetValue(),
				"unicode user %q must get deterministic result", userID)
			// At 100% rollout, all users must get "true".
			assert.Equal(t, "true", eval1.Msg.GetValue())
		})
	}
}

// TestEvaluateFlag_EmptyUserID verifies that an empty user_id returns
// CodeInvalidArgument (validated at evaluate.go:21).
func TestEvaluateFlag_EmptyUserID(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "empty-userid-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)

	_, err = client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
		FlagId: created.Msg.GetFlagId(),
		UserId: "",
	}))
	assert.Error(t, err)
	assert.Equal(t, connect.CodeInvalidArgument, connect.CodeOf(err))
}

// TestEvaluateFlag_100PercentAllUsers verifies that 200 distinct users at
// 100% rollout all receive "true".
func TestEvaluateFlag_100PercentAllUsers(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "all-users-flag",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 1.0,
		},
	}))
	require.NoError(t, err)
	flagID := created.Msg.GetFlagId()

	for i := 0; i < 200; i++ {
		eval, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
			FlagId: flagID,
			UserId: fmt.Sprintf("user_%d", i),
		}))
		require.NoError(t, err)
		assert.Equal(t, "true", eval.Msg.GetValue(),
			"user_%d at 100%% rollout must get true", i)
	}
}
