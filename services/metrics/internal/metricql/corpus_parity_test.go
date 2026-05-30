package metricql

import (
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"testing"
)

// fixture mirrors one entry in test-vectors/metricql_corpus.json.
type corpusFixture struct {
	Name               string   `json:"name"`
	Source             string   `json:"source"`
	Valid              bool     `json:"valid"`
	ExpectedRefs       []string `json:"expected_refs,omitempty"`
	ExpectedErrorCount *int     `json:"expected_error_count,omitempty"`
}

// TestCorpusParity loads the shared golden corpus and asserts that the Go
// MetricQL implementation accepts/rejects every fixture identically to the
// Rust implementation (tested in
// crates/experimentation-management/tests/metricql_corpus_parity.rs).
//
// For valid fixtures:
//   - Parse must succeed.
//   - Analyze must succeed (known set populated from expected_refs).
//   - CollectMetricRefs must return the expected set (order-independent).
//
// For invalid fixtures:
//   - Either Parse OR Analyze must return an error.
//
// Note: Go's Analyze returns a single error (not a Vec). For invalid fixtures
// with expected_error_count > 1, the Go side can only assert "errored" — the
// Rust side asserts the exact diagnostic count. This asymmetry is intentional.
func TestCorpusParity(t *testing.T) {
	root := repoRoot(t)
	raw, err := os.ReadFile(filepath.Join(root, "test-vectors/metricql_corpus.json"))
	if err != nil {
		t.Fatalf("read corpus: %v", err)
	}

	var fixtures []corpusFixture
	if err := json.Unmarshal(raw, &fixtures); err != nil {
		t.Fatalf("parse corpus JSON: %v", err)
	}
	if len(fixtures) < 30 {
		t.Fatalf("corpus shrank unexpectedly: %d fixtures", len(fixtures))
	}

	for _, f := range fixtures {
		f := f // capture for t.Run closure
		t.Run(f.Name, func(t *testing.T) {
			node, parseErr := Parse(f.Source)

			if !f.Valid {
				// Expect either parse or analyze to reject the input.
				if parseErr != nil {
					return // parse rejected it — good
				}
				// Parse succeeded; run analyzer with empty known set so any
				// semantic violations surface.
				ctx := AnalyzeContext{KnownMetricIDs: map[string]bool{}}
				analyzeErr := Analyze(node, ctx)
				if analyzeErr == nil {
					t.Fatalf("fixture %q: expected an error, but parsed and analyzed cleanly", f.Name)
				}
				return // analyze rejected it — good
			}

			// Valid fixture: parse must succeed.
			if parseErr != nil {
				t.Fatalf("fixture %q: expected valid, parse failed: %v", f.Name, parseErr)
			}

			// Build the known set from expected_refs so existence checks pass.
			known := map[string]bool{}
			for _, r := range f.ExpectedRefs {
				known[r] = true
			}
			ctx := AnalyzeContext{KnownMetricIDs: known}
			if err := Analyze(node, ctx); err != nil {
				t.Fatalf("fixture %q: expected valid, analyze failed: %v", f.Name, err)
			}

			// Verify extracted @metric_refs match corpus.
			got := CollectMetricRefs(node)
			sort.Strings(got)
			want := append([]string(nil), f.ExpectedRefs...)
			sort.Strings(want)
			if !stringSliceEqual(got, want) {
				t.Fatalf("fixture %q: refs mismatch — want %v, got %v", f.Name, want, got)
			}
		})
	}
}

func stringSliceEqual(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// repoRoot walks up from the package directory to find the repository root.
// Package lives at services/metrics/internal/metricql/ — four levels up.
func repoRoot(t *testing.T) string {
	t.Helper()
	wd, err := os.Getwd()
	if err != nil {
		t.Fatalf("getwd: %v", err)
	}
	return filepath.Clean(filepath.Join(wd, "../../../.."))
}
