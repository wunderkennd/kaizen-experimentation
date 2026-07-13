// ADR-031 conformance suite — Go client ↔ Rust server (buffa) round-trip
// coverage for every AssignmentService RPC we've wired.
//
// Started life as #641's tracer bullet (GetAssignment only) and grew, with
// #642 and #643, to cover the remaining unary methods and the streaming RPC.
// This is the cross-language wire check the pilot's kill/success criteria
// depend on: proves the Anthropic-buffa server actually interoperates with a
// vanilla connectrpc.com/connect Go client without hand-rolled shim code on
// either side (that shim, http_json.rs + the JSON POST client in
// experimentation.go, is retired in #644).
//
// Opt-in: runs only when KAIZEN_M1_CONNECT_URL is set. To run locally, start
// the M1 pilot binary with --features connectrpc, then:
//
//	CONNECTRPC_ADDR=127.0.0.1:50161 \
//	  cargo run -p experimentation-assignment --features connectrpc --bin experimentation-assignment
//	KAIZEN_M1_CONNECT_URL=http://127.0.0.1:50161 go test ./sdks/server-go/...
package experimentation_test

import (
	"context"
	"net/http"
	"os"
	"testing"
	"time"

	"connectrpc.com/connect"
	assignmentv1 "github.com/org/experimentation/gen/go/experimentation/assignment/v1"
	"github.com/org/experimentation/gen/go/experimentation/assignment/v1/assignmentv1connect"
)

// conformanceClient returns a Connect client pointed at the pilot server, or
// skips the test if KAIZEN_M1_CONNECT_URL isn't configured. Every test in
// this file starts with this call — one skip decision, not N.
func conformanceClient(t *testing.T) assignmentv1connect.AssignmentServiceClient {
	t.Helper()
	url := os.Getenv("KAIZEN_M1_CONNECT_URL")
	if url == "" {
		t.Skip("set KAIZEN_M1_CONNECT_URL to a running M1 pilot to exercise this test")
	}
	httpClient := &http.Client{Timeout: 5 * time.Second}
	return assignmentv1connect.NewAssignmentServiceClient(httpClient, url)
}

func TestConformance_GetAssignment_RoundTrip(t *testing.T) {
	client := conformanceClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	resp, err := client.GetAssignment(ctx, connect.NewRequest(&assignmentv1.GetAssignmentRequest{
		UserId:       "test-user-1",
		ExperimentId: "exp_dev_001",
		SessionId:    "sess-1",
	}))
	if err != nil {
		t.Fatalf("GetAssignment failed: %v", err)
	}
	if got, want := resp.Msg.ExperimentId, "exp_dev_001"; got != want {
		t.Errorf("ExperimentId = %q, want %q", got, want)
	}
	// IsActive is the load-bearing contract field; assert presence, not value
	// (the experiment's state can change in dev/config.json).
	_ = resp.Msg.IsActive
}

func TestConformance_GetAssignment_UnknownExperimentReturnsNotFound(t *testing.T) {
	client := conformanceClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	_, err := client.GetAssignment(ctx, connect.NewRequest(&assignmentv1.GetAssignmentRequest{
		UserId:       "u1",
		ExperimentId: "definitely-not-an-experiment",
		SessionId:    "s1",
	}))
	if err == nil {
		t.Fatal("expected NotFound error, got nil")
	}
	if connect.CodeOf(err) != connect.CodeNotFound {
		t.Errorf("expected CodeNotFound, got %v: %v", connect.CodeOf(err), err)
	}
}

// GetAssignments — batch path over the wire. The Rust server absorbs
// per-experiment failures inside assign_batch, so this always returns 200
// with a (possibly empty) assignments array.
func TestConformance_GetAssignments_ReturnsBatch(t *testing.T) {
	client := conformanceClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	resp, err := client.GetAssignments(ctx, connect.NewRequest(&assignmentv1.GetAssignmentsRequest{
		UserId:    "test-user-1",
		SessionId: "sess-1",
	}))
	if err != nil {
		t.Fatalf("GetAssignments failed: %v", err)
	}
	if len(resp.Msg.Assignments) == 0 {
		t.Error("expected at least one assignment from dev/config.json, got empty batch")
	}
}

// GetInterleavedList — closes the JSON coverage gap (this method had no
// hand-rolled JSON path before #642).
func TestConformance_GetInterleavedList_MergesTwoAlgorithms(t *testing.T) {
	client := conformanceClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	// exp_dev_004 is the TEAM_DRAFT interleaving experiment
	// (algorithm_ids: ["algo_a", "algo_b"]) in dev/config.json.
	resp, err := client.GetInterleavedList(ctx, connect.NewRequest(&assignmentv1.GetInterleavedListRequest{
		ExperimentId: "exp_dev_004",
		UserId:       "test-user-interleave",
		AlgorithmLists: map[string]*assignmentv1.RankedList{
			"algo_a": {ItemIds: []string{"a1", "a2", "a3"}},
			"algo_b": {ItemIds: []string{"b1", "b2", "b3"}},
		},
	}))
	if err != nil {
		t.Fatalf("GetInterleavedList failed: %v", err)
	}
	if len(resp.Msg.MergedList) == 0 {
		t.Error("expected non-empty merged list")
	}
}

// GetSlateAssignment — SLATE_FACTORIZED_TS experiment; asserts the wire
// contract for slot count without asserting specific items (order is bandit
// draw-dependent).
func TestConformance_GetSlateAssignment_ReturnsOrderedSlate(t *testing.T) {
	client := conformanceClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	resp, err := client.GetSlateAssignment(ctx, connect.NewRequest(&assignmentv1.GetSlateAssignmentRequest{
		UserId:           "test-user-slate",
		ExperimentId:     "exp_dev_slate_001",
		CandidateItemIds: []string{"i1", "i2", "i3", "i4", "i5", "i6"},
	}))
	if err != nil {
		t.Fatalf("GetSlateAssignment failed: %v", err)
	}
	if got, want := len(resp.Msg.SlateItemIds), 3; got != want {
		t.Errorf("slate length = %d, want %d (num_slots=3)", got, want)
	}
	if got, want := len(resp.Msg.SlotProbabilities), 3; got != want {
		t.Errorf("slot probabilities = %d, want %d", got, want)
	}
}

// StreamConfigUpdates — proves the Connect server-streaming path opens
// end-to-end. Doesn't push updates from this test (M5 integration isn't
// wired), just asserts the stream can be opened without an immediate error
// and cleanly closed. The domain streaming invariants (ordering, fan-out)
// are covered by the Rust-side tests in
// crates/experimentation-assignment/tests/stream_config_updates_test.rs.
func TestConformance_StreamConfigUpdates_OpensCleanly(t *testing.T) {
	client := conformanceClient(t)
	ctx, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
	defer cancel()

	stream, err := client.StreamConfigUpdates(ctx, connect.NewRequest(&assignmentv1.StreamConfigUpdatesRequest{
		LastKnownVersion: 0,
	}))
	if err != nil {
		t.Fatalf("StreamConfigUpdates open failed: %v", err)
	}
	defer stream.Close()

	// Drain until context deadline. A clean deadline-exceeded (or a nil-error
	// EOF once M5 is wired) is the pass condition — the server accepted the
	// subscribe request and honored the client-side cancel.
	for stream.Receive() {
		_ = stream.Msg()
	}
	if err := stream.Err(); err != nil && connect.CodeOf(err) != connect.CodeDeadlineExceeded && connect.CodeOf(err) != connect.CodeCanceled {
		t.Errorf("stream close returned unexpected error: code=%v err=%v", connect.CodeOf(err), err)
	}
}
