package handlers

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/org/experimentation-platform/services/flags/internal/store"
	flagsv1 "github.com/org/experimentation/gen/go/experimentation/flags/v1"
	"github.com/org/experimentation/gen/go/experimentation/flags/v1/flagsv1connect"
)

// setupBenchmark creates a ConnectRPC test server with a MockStore, suitable for benchmarks.
// Returns the client and mock store. The server is closed when b.Cleanup runs.
// Uses a pooled HTTP client to avoid port exhaustion under parallel load.
func setupBenchmark(b *testing.B) (flagsv1connect.FeatureFlagServiceClient, *store.MockStore) {
	b.Helper()
	mockStore := store.NewMockStore()
	svc := NewFlagService(mockStore)
	mux := http.NewServeMux()
	path, handler := flagsv1connect.NewFeatureFlagServiceHandler(svc)
	mux.Handle(path, handler)
	server := httptest.NewServer(mux)
	b.Cleanup(server.Close)

	httpClient := &http.Client{
		Transport: &http.Transport{
			MaxIdleConns:        256,
			MaxIdleConnsPerHost: 256,
			IdleConnTimeout:     90 * time.Second,
			DialContext: (&net.Dialer{
				Timeout:   5 * time.Second,
				KeepAlive: 30 * time.Second,
			}).DialContext,
		},
	}
	client := flagsv1connect.NewFeatureFlagServiceClient(httpClient, server.URL)
	return client, mockStore
}

// createBenchFlag creates a boolean flag with the given rollout percentage and returns its ID.
func createBenchFlag(b *testing.B, client flagsv1connect.FeatureFlagServiceClient, name string, rollout float64) string {
	b.Helper()
	resp, err := client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              name,
			Type:              flagsv1.FlagType_FLAG_TYPE_BOOLEAN,
			DefaultValue:      "false",
			Enabled:           true,
			RolloutPercentage: rollout,
		},
	}))
	if err != nil {
		b.Fatalf("create flag %q: %v", name, err)
	}
	return resp.Msg.GetFlagId()
}

// BenchmarkEvaluateFlag benchmarks a single EvaluateFlag RPC through the full
// ConnectRPC HTTP stack (server + handler + hash). Target: < 10ms/op.
func BenchmarkEvaluateFlag(b *testing.B) {
	client, _ := setupBenchmark(b)
	flagID := createBenchFlag(b, client, "bench-eval", 0.5)
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
			FlagId: flagID,
			UserId: "user_123",
		}))
		if err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkEvaluateFlag_Parallel simulates concurrent EvaluateFlag calls with
// unique user IDs per goroutine, measuring throughput under load.
func BenchmarkEvaluateFlag_Parallel(b *testing.B) {
	client, _ := setupBenchmark(b)
	flagID := createBenchFlag(b, client, "bench-eval-parallel", 0.5)
	ctx := context.Background()

	b.ResetTimer()
	b.RunParallel(func(pb *testing.PB) {
		i := 0
		for pb.Next() {
			_, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
				FlagId: flagID,
				UserId: fmt.Sprintf("user_%d", i),
			}))
			if err != nil {
				b.Fatal(err)
			}
			i++
		}
	})
}

// BenchmarkEvaluateFlags_Bulk benchmarks the bulk EvaluateFlags RPC with 50
// enabled flags. Measures GetAllEnabledFlags + N hash calls.
func BenchmarkEvaluateFlags_Bulk(b *testing.B) {
	client, _ := setupBenchmark(b)
	for i := 0; i < 50; i++ {
		createBenchFlag(b, client, fmt.Sprintf("bench-bulk-%d", i), 0.5)
	}
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		resp, err := client.EvaluateFlags(ctx, connect.NewRequest(&flagsv1.EvaluateFlagsRequest{
			UserId: "user_bulk_bench",
		}))
		if err != nil {
			b.Fatal(err)
		}
		if len(resp.Msg.GetEvaluations()) != 50 {
			b.Fatalf("expected 50 evaluations, got %d", len(resp.Msg.GetEvaluations()))
		}
	}
}

// BenchmarkEvaluateFlag_VariantSelection benchmarks flag evaluation with 5
// variants, testing the variant routing loop overhead.
func BenchmarkEvaluateFlag_VariantSelection(b *testing.B) {
	client, _ := setupBenchmark(b)

	resp, err := client.CreateFlag(context.Background(), connect.NewRequest(&flagsv1.CreateFlagRequest{
		Flag: &flagsv1.Flag{
			Name:              "bench-variants",
			Type:              flagsv1.FlagType_FLAG_TYPE_STRING,
			DefaultValue:      "control",
			Enabled:           true,
			RolloutPercentage: 1.0,
			Variants: []*flagsv1.FlagVariant{
				{Value: "variant-a", TrafficFraction: 0.2},
				{Value: "variant-b", TrafficFraction: 0.2},
				{Value: "variant-c", TrafficFraction: 0.2},
				{Value: "variant-d", TrafficFraction: 0.2},
				{Value: "variant-e", TrafficFraction: 0.2},
			},
		},
	}))
	if err != nil {
		b.Fatalf("create variant flag: %v", err)
	}
	flagID := resp.Msg.GetFlagId()
	ctx := context.Background()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := client.EvaluateFlag(ctx, connect.NewRequest(&flagsv1.EvaluateFlagRequest{
			FlagId: flagID,
			UserId: fmt.Sprintf("user_%d", i),
		}))
		if err != nil {
			b.Fatal(err)
		}
	}
}
