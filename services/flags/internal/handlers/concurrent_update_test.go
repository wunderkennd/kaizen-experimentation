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

// TestConcurrentFlagUpdates_50Writers verifies that 50 goroutines can call
// UpdateFlag on the same flag simultaneously without race conditions.
// Run with: go test -race -run=TestConcurrentFlagUpdates_50Writers
func TestConcurrentFlagUpdates_50Writers(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	// Create the flag to be updated concurrently.
	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "concurrent-update-target",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.1,
		},
	}))
	require.NoError(t, err)
	flagID := created.Msg.GetFlagId()

	const writers = 50
	var wg sync.WaitGroup
	wg.Add(writers)

	errs := make([]error, writers)

	for i := 0; i < writers; i++ {
		go func(i int) {
			defer wg.Done()
			// Each writer sets a unique rollout percentage.
			rollout := float64(i+1) / float64(writers+1)
			_, err := client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
				Flag: &flagsv1.Flag{
					FlagId:            flagID,
					Name:              "concurrent-update-target",
					Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
					DefaultValue:      "false",
					Enabled:           true,
					RolloutPercentage: rollout,
				},
			}))
			errs[i] = err
		}(i)
	}

	wg.Wait()

	// All 50 updates must succeed (no errors).
	for i, err := range errs {
		assert.NoError(t, err, "writer %d failed", i)
	}

	// Final flag state must be valid.
	final, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{
		FlagId: flagID,
	}))
	require.NoError(t, err)
	assert.Equal(t, flagID, final.Msg.GetFlagId())
	assert.True(t, final.Msg.GetRolloutPercentage() > 0.0, "rollout must be positive")
	assert.True(t, final.Msg.GetRolloutPercentage() < 1.0, "rollout must be < 1.0")
}

// TestConcurrentReadWrite_Mixed simulates a realistic workload: 50 concurrent
// readers (EvaluateFlag) and 10 concurrent writers (UpdateFlag) on the same flag.
// Run with: go test -race -run=TestConcurrentReadWrite_Mixed
func TestConcurrentReadWrite_Mixed(t *testing.T) {
	client, _ := setupTest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "mixed-rw-target",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)
	flagID := created.Msg.GetFlagId()

	const readers = 50
	const writers = 10
	var wg sync.WaitGroup
	wg.Add(readers + writers)

	readErrs := make([]error, readers)
	readVals := make([]string, readers)
	writeErrs := make([]error, writers)

	// Launch readers.
	for i := 0; i < readers; i++ {
		go func(i int) {
			defer wg.Done()
			resp, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
				FlagId: flagID,
				UserId: fmt.Sprintf("reader_%d", i),
			}))
			readErrs[i] = err
			if err == nil {
				readVals[i] = resp.Msg.GetValue()
			}
		}(i)
	}

	// Launch writers.
	for i := 0; i < writers; i++ {
		go func(i int) {
			defer wg.Done()
			rollout := 0.1 + float64(i)*0.08 // 0.1 to 0.82
			_, err := client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
				Flag: &flagsv1.Flag{
					FlagId:            flagID,
					Name:              "mixed-rw-target",
					Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
					DefaultValue:      "false",
					Enabled:           true,
					RolloutPercentage: rollout,
				},
			}))
			writeErrs[i] = err
		}(i)
	}

	wg.Wait()

	// No reader panics or errors.
	for i, err := range readErrs {
		assert.NoError(t, err, "reader %d failed", i)
	}

	// All reader values are valid boolean strings.
	for i, v := range readVals {
		if readErrs[i] == nil {
			assert.Contains(t, []string{"true", "false"}, v,
				"reader %d got invalid value %q", i, v)
		}
	}

	// No writer errors.
	for i, err := range writeErrs {
		assert.NoError(t, err, "writer %d failed", i)
	}
}
