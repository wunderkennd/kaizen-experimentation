package metricql

import (
	"sort"
	"strings"
	"testing"
)

func TestAnalyze_HappyPath(t *testing.T) {
	ctx := AnalyzeContext{KnownMetricIDs: map[string]bool{"watch_time": true, "stream_count": true}}
	cases := []string{
		"mean(heartbeat.value)",
		"sum(purchase.amount)",
		"count(stream_start)",
		"count_distinct(purchase.product_id)",
		"proportion(stream_start)",
		"percentile(95)(latency.value)",
		"mean(x.v) within 7 days of exposure",
		"mean(x.v) where platform = 'mobile'",
		"mean(x.v) where p in ['a', 'b'] and country != 'us'",
		"0.7 * @watch_time + 0.3 * @stream_count",
		"ratio(@watch_time, @stream_count)",
		"-@watch_time + @stream_count",
	}
	for _, src := range cases {
		t.Run(src, func(t *testing.T) {
			root, err := Parse(src)
			if err != nil {
				t.Fatalf("parse: %v", err)
			}
			if err := Analyze(root, ctx); err != nil {
				t.Errorf("analyze should accept %q, rejected: %v", src, err)
			}
		})
	}
}

func TestAnalyze_RejectsBareRefOrLiteral(t *testing.T) {
	ctx := AnalyzeContext{KnownMetricIDs: map[string]bool{"watch_time": true}}
	cases := []struct {
		src, msg string
	}{
		{"@watch_time", "not a bare metric reference"},
		{"42", "not a bare literal"},
		{"42.5", "not a bare literal"},
	}
	for _, tc := range cases {
		t.Run(tc.src, func(t *testing.T) {
			root, err := Parse(tc.src)
			if err != nil {
				t.Fatalf("parse: %v", err)
			}
			err = Analyze(root, ctx)
			if err == nil {
				t.Fatalf("expected analyze rejection")
			}
			if !strings.Contains(err.Error(), tc.msg) {
				t.Errorf("error %q does not contain %q", err.Error(), tc.msg)
			}
		})
	}
}

func TestAnalyze_RejectsCountWithField(t *testing.T) {
	// count() operates on event presence; count(heartbeat.value) is nonsense.
	src := "count(heartbeat.value)"
	root, err := Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	err = Analyze(root, AnalyzeContext{})
	if err == nil {
		t.Fatal("expected rejection")
	}
	if !strings.Contains(err.Error(), "event presence") {
		t.Errorf("error should mention 'event presence': %v", err)
	}
}

func TestAnalyze_RejectsProportionWithField(t *testing.T) {
	src := "proportion(heartbeat.value)"
	root, err := Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	err = Analyze(root, AnalyzeContext{})
	if err == nil {
		t.Fatal("expected rejection")
	}
	if !strings.Contains(err.Error(), "event presence") {
		t.Errorf("error should mention 'event presence': %v", err)
	}
}

func TestAnalyze_RejectsMeanWithoutField(t *testing.T) {
	// mean() needs a value to average; mean(stream_start) is nonsense.
	src := "mean(stream_start)"
	root, err := Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	err = Analyze(root, AnalyzeContext{})
	if err == nil {
		t.Fatal("expected rejection")
	}
	if !strings.Contains(err.Error(), "requires a value field") {
		t.Errorf("error should mention 'requires a value field': %v", err)
	}
}

func TestAnalyze_RejectsCountDistinctWithoutField(t *testing.T) {
	src := "count_distinct(stream_start)"
	root, err := Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	err = Analyze(root, AnalyzeContext{})
	if err == nil {
		t.Fatal("expected rejection")
	}
	if !strings.Contains(err.Error(), "requires a value field") {
		t.Errorf("error should mention 'requires a value field': %v", err)
	}
}

func TestAnalyze_RejectsUnknownMetricRef(t *testing.T) {
	src := "@unknown_metric + @other"
	root, err := Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	ctx := AnalyzeContext{KnownMetricIDs: map[string]bool{"watch_time": true}}
	err = Analyze(root, ctx)
	if err == nil {
		t.Fatal("expected rejection")
	}
	if !strings.Contains(err.Error(), "unknown metric reference @unknown_metric") {
		t.Errorf("error should mention unknown metric reference: %v", err)
	}
}

func TestAnalyze_NilKnownMetricIDs_SkipsExistenceCheck(t *testing.T) {
	// When KnownMetricIDs is nil (M5 first-creation case), existence checks are skipped.
	// Syntactic validation of the ID still happens.
	src := "@some_metric + @other_metric"
	root, err := Parse(src)
	if err != nil {
		t.Fatal(err)
	}
	if err := Analyze(root, AnalyzeContext{KnownMetricIDs: nil}); err != nil {
		t.Errorf("with nil KnownMetricIDs, analyzer should accept unknown refs, got: %v", err)
	}
}

func TestAnalyze_CollectMetricRefs_Deduplicates(t *testing.T) {
	// "@a + @b + @a" should yield {"a", "b"} once each.
	root, err := Parse("@a + @b + @a")
	if err != nil {
		t.Fatal(err)
	}
	refs := CollectMetricRefs(root)
	sort.Strings(refs)
	want := []string{"a", "b"}
	if len(refs) != len(want) {
		t.Fatalf("got %v, want %v", refs, want)
	}
	for i, r := range refs {
		if r != want[i] {
			t.Errorf("refs[%d]: got %q, want %q", i, r, want[i])
		}
	}
}

func TestAnalyze_CollectMetricRefs_RatioCounted(t *testing.T) {
	root, err := Parse("ratio(@num, @den)")
	if err != nil {
		t.Fatal(err)
	}
	refs := CollectMetricRefs(root)
	sort.Strings(refs)
	want := []string{"den", "num"}
	if len(refs) != 2 || refs[0] != want[0] || refs[1] != want[1] {
		t.Errorf("ratio refs: got %v, want %v", refs, want)
	}
}

func TestAnalyze_PercentileRangeDefenseInDepth(t *testing.T) {
	// Parser rejects out-of-range, but analyzer should also reject if invoked
	// on a synthetic AST with bad Percentile (e.g., from a future M5 importer).
	root := &Aggregation{
		Func:       AggPercentile,
		Percentile: 150.0,
		Source:     Source{EventType: "x", Field: "v"},
	}
	if err := Analyze(root, AnalyzeContext{}); err == nil {
		t.Fatal("expected rejection of percentile=150")
	}
}
