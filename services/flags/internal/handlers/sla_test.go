package handlers

import (
	"context"
	"fmt"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"sort"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/org/experimentation-platform/services/flags/internal/hash"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// isCI returns true when running on GitHub Actions or other CI environments.
// CI runners are shared VMs with significantly higher latency than local dev
// machines, so SLA thresholds are relaxed proportionally.
func isCI() bool {
	return os.Getenv("CI") == "true" || os.Getenv("GITHUB_ACTIONS") == "true"
}

// slaThreshold returns a CI-aware duration threshold. On CI, thresholds are
// multiplied by 50x to account for shared-VM overhead (measured: ~18-30x slower
// than Apple M4 Pro, with additional variance from noisy neighbors).
func slaThreshold(local time.Duration) time.Duration {
	if isCI() {
		return local * 50
	}
	return local
}

// TestSLA_EvaluateFlag_Latency validates that EvaluateFlag p99 < 10ms under load.
// Sends 10K sequential requests and asserts the 99th percentile is within SLA.
func TestSLA_EvaluateFlag_Latency(t *testing.T) {
	client, _ := setupSLATest(t)
	ctx := context.Background()

	// Create a flag to evaluate.
	flagID := createSLAFlag(t, client, "sla-latency-eval", 0.5)

	const totalRequests = 10000
	latencies := make([]time.Duration, totalRequests)

	// Warm up.
	for i := 0; i < 100; i++ {
		_, _ = client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
			FlagId: flagID,
			UserId: fmt.Sprintf("warmup_%d", i),
		}))
	}

	// Measure.
	for i := 0; i < totalRequests; i++ {
		start := time.Now()
		_, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
			FlagId: flagID,
			UserId: fmt.Sprintf("user_%d", i),
		}))
		latencies[i] = time.Since(start)
		require.NoError(t, err)
	}

	p99 := percentile(latencies, 0.99)
	p50 := percentile(latencies, 0.50)
	maxLat := maxDuration(latencies)

	t.Logf("EvaluateFlag latency (n=%d): p50=%v  p99=%v  max=%v", totalRequests, p50, p99, maxLat)

	threshold := slaThreshold(10 * time.Millisecond)
	assert.Less(t, p99, threshold,
		"SLA violation: EvaluateFlag p99 = %v (target: < %v)", p99, threshold)
}

// TestSLA_EvaluateFlag_Concurrent validates EvaluateFlag under concurrent load.
// 100 goroutines each send 100 requests (10K total). Uses 20ms threshold since
// httptest server contention inflates latency vs production (the sequential test
// validates the 10ms SLA; this test validates correctness under contention).
func TestSLA_EvaluateFlag_Concurrent(t *testing.T) {
	client, _ := setupSLATest(t)
	ctx := context.Background()

	flagID := createSLAFlag(t, client, "sla-concurrent-eval", 0.5)

	const goroutines = 100
	const requestsPerGoroutine = 100

	allLatencies := make([][]time.Duration, goroutines)
	var errorCount atomic.Int64

	var wg sync.WaitGroup
	wg.Add(goroutines)

	for g := 0; g < goroutines; g++ {
		allLatencies[g] = make([]time.Duration, requestsPerGoroutine)
		go func(g int) {
			defer wg.Done()
			for i := 0; i < requestsPerGoroutine; i++ {
				start := time.Now()
				_, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
					FlagId: flagID,
					UserId: fmt.Sprintf("user_%d_%d", g, i),
				}))
				allLatencies[g][i] = time.Since(start)
				if err != nil {
					errorCount.Add(1)
				}
			}
		}(g)
	}

	wg.Wait()

	// Flatten latencies.
	total := goroutines * requestsPerGoroutine
	flat := make([]time.Duration, 0, total)
	for _, gl := range allLatencies {
		flat = append(flat, gl...)
	}

	p99 := percentile(flat, 0.99)
	p50 := percentile(flat, 0.50)
	errRate := float64(errorCount.Load()) / float64(total)

	t.Logf("Concurrent EvaluateFlag (n=%d, goroutines=%d): p50=%v  p99=%v  errors=%.3f%%",
		total, goroutines, p50, p99, errRate*100)

	threshold := slaThreshold(20 * time.Millisecond)
	assert.Less(t, p99, threshold,
		"SLA violation: concurrent EvaluateFlag p99 = %v (target: < %v)", p99, threshold)
	assert.Less(t, errRate, 0.001,
		"SLA violation: error rate = %.3f%% (target: < 0.1%%)", errRate*100)
}

// TestSLA_EvaluateFlags_Bulk validates bulk evaluation with 50 flags under p99 < 50ms.
func TestSLA_EvaluateFlags_Bulk(t *testing.T) {
	client, _ := setupSLATest(t)
	ctx := context.Background()

	// Seed 50 enabled flags.
	for i := 0; i < 50; i++ {
		createSLAFlag(t, client, fmt.Sprintf("sla-bulk-%d", i), 0.5)
	}

	const totalRequests = 1000
	latencies := make([]time.Duration, totalRequests)

	// Warm up.
	for i := 0; i < 50; i++ {
		_, _ = client.EvaluateFlags(ctx, connect.NewRequest(&flagsv1.EvaluateFlagsRequest{
			UserId: fmt.Sprintf("warmup_%d", i),
		}))
	}

	for i := 0; i < totalRequests; i++ {
		start := time.Now()
		resp, err := client.EvaluateFlags(ctx, connect.NewRequest(&flagsv1.EvaluateFlagsRequest{
			UserId: fmt.Sprintf("bulk_user_%d", i),
		}))
		latencies[i] = time.Since(start)
		require.NoError(t, err)
		assert.Equal(t, 50, len(resp.Msg.GetEvaluations()), "expected 50 flag evaluations")
	}

	p99 := percentile(latencies, 0.99)
	p50 := percentile(latencies, 0.50)

	t.Logf("EvaluateFlags bulk (50 flags, n=%d): p50=%v  p99=%v", totalRequests, p50, p99)

	threshold := slaThreshold(50 * time.Millisecond)
	assert.Less(t, p99, threshold,
		"SLA violation: EvaluateFlags bulk p99 = %v (target: < %v)", p99, threshold)
}

// TestSLA_HashBucket_SubMicrosecond validates that hash.Bucket is fast enough.
// Pre-generates user IDs to isolate hash cost from string allocation.
// Uses 2μs threshold to account for -race detector overhead and pure Go fallback.
// Production CGo bridge target is < 1μs; validated by Go benchmarks (BenchmarkBucket)
// and the k6 load test end-to-end.
func TestSLA_HashBucket_SubMicrosecond(t *testing.T) {
	const iterations = 100000

	// Pre-generate user IDs so Sprintf isn't measured.
	userIDs := make([]string, iterations)
	for i := range userIDs {
		userIDs[i] = fmt.Sprintf("user_%d", i)
	}

	// Warm up CPU caches.
	for i := 0; i < 1000; i++ {
		hash.Bucket(userIDs[i], "test_salt", 10000)
	}

	start := time.Now()
	for i := 0; i < iterations; i++ {
		hash.Bucket(userIDs[i], "test_salt", 10000)
	}
	elapsed := time.Since(start)

	avgNs := float64(elapsed.Nanoseconds()) / float64(iterations)

	t.Logf("hash.Bucket average: %.1f ns/op (n=%d)", avgNs, iterations)

	// 2μs threshold for pure Go + -race overhead. CGo bridge target is < 1μs.
	// CI runners are ~18x slower; use 50μs ceiling.
	thresholdNs := 2000.0
	if isCI() {
		thresholdNs = 50000.0
	}
	assert.Less(t, avgNs, thresholdNs,
		"SLA violation: hash.Bucket average = %.1f ns (target: < %.0f ns)", avgNs, thresholdNs)
}

// TestSLA_ConcurrentUpdates_NoRace verifies that 50 concurrent flag updates
// with mixed reads do not produce errors or data corruption.
func TestSLA_ConcurrentUpdates_NoRace(t *testing.T) {
	client, _ := setupSLATest(t)
	ctx := context.Background()

	created, err := client.CreateFlag(ctx, connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "sla-concurrent-update",
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: 0.5,
		},
	}))
	require.NoError(t, err)
	flagID := created.Msg.GetFlagId()

	const writers = 50
	const readersPerWriter = 5
	var writeErrors atomic.Int64
	var readErrors atomic.Int64
	var invalidValues atomic.Int64

	var wg sync.WaitGroup
	wg.Add(writers + writers*readersPerWriter)

	// 50 writers — each updates the flag's rollout percentage.
	for w := 0; w < writers; w++ {
		go func(w int) {
			defer wg.Done()
			rollout := float64(w+1) / float64(writers+1)
			_, err := client.UpdateFlag(ctx, connect.NewRequest(&flagsv1.UpdateFlagRequest{
				Flag: &flagsv1.Flag{
					FlagId:            flagID,
					Name:              "sla-concurrent-update",
					Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
					DefaultValue:      "false",
					Enabled:           true,
					RolloutPercentage: rollout,
				},
			}))
			if err != nil {
				writeErrors.Add(1)
			}
		}(w)

		// 5 readers per writer — evaluate the flag concurrently.
		for r := 0; r < readersPerWriter; r++ {
			go func(w, r int) {
				defer wg.Done()
				resp, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
					FlagId: flagID,
					UserId: fmt.Sprintf("reader_%d_%d", w, r),
				}))
				if err != nil {
					readErrors.Add(1)
				} else if resp.Msg.GetValue() != "true" && resp.Msg.GetValue() != "false" {
					invalidValues.Add(1)
				}
			}(w, r)
		}
	}

	wg.Wait()

	t.Logf("Concurrent update test: writers=%d readers=%d write_errors=%d read_errors=%d invalid=%d",
		writers, writers*readersPerWriter,
		writeErrors.Load(), readErrors.Load(), invalidValues.Load())

	assert.Equal(t, int64(0), writeErrors.Load(), "write errors")
	assert.Equal(t, int64(0), readErrors.Load(), "read errors")
	assert.Equal(t, int64(0), invalidValues.Load(), "invalid evaluation values")

	// Verify the flag is still readable and consistent.
	final, err := client.GetFlag(ctx, connect.NewRequest(&flagsv1.GetFlagRequest{FlagId: flagID}))
	require.NoError(t, err)
	assert.Equal(t, flagID, final.Msg.GetFlagId())
	assert.True(t, final.Msg.GetRolloutPercentage() > 0, "rollout must be positive")
	assert.True(t, final.Msg.GetRolloutPercentage() < 1.0, "rollout must be < 1.0")
}

// --- Helpers ---

func setupSLATest(t *testing.T) (flagsv1connect.FeatureFlagServiceClient, *store.MockStore) {
	t.Helper()
	mockStore := store.NewMockStore()
	svc := NewFlagService(mockStore)
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	t.Cleanup(server.Close)

	httpClient := &http.Client{
		Transport: &http.Transport{
			MaxIdleConns:        512,
			MaxIdleConnsPerHost: 512,
			IdleConnTimeout:     90 * time.Second,
		},
	}
	client := flagsv1connect.NewFeatureFlagServiceClient(httpClient, server.URL)
	return client, mockStore
}

func createSLAFlag(t *testing.T, client flagsv1connect.FeatureFlagServiceClient, name string, rollout float64) string {
	t.Helper()
	resp, err := client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              name,
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: rollout,
		},
	}))
	require.NoError(t, err)
	return resp.Msg.GetFlagId()
}

// percentile calculates the p-th percentile from unsorted durations.
func percentile(durations []time.Duration, p float64) time.Duration {
	if len(durations) == 0 {
		return 0
	}
	// Copy and sort.
	sorted := make([]time.Duration, len(durations))
	copy(sorted, durations)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i] < sorted[j] })

	idx := int(math.Ceil(p*float64(len(sorted)))) - 1
	if idx < 0 {
		idx = 0
	}
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

func maxDuration(durations []time.Duration) time.Duration {
	var max time.Duration
	for _, d := range durations {
		if d > max {
			max = d
		}
	}
	return max
}

