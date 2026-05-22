package metricql

import (
	"flag"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

var updateGolden = flag.Bool("update", false, "update golden SQL files in testdata/")

// testContext builds a CompileContext with a generous KnownMetricIDs map
// covering every @ref used across the golden-file suite.
func testContext(metricID string) CompileContext {
	return CompileContext{
		ExperimentID:    "exp_test",
		ComputationDate: "2026-05-18",
		MetricID:        metricID,
		KnownMetricIDs: map[string]bool{
			"watch_time":     true,
			"ctr":            true,
			"total_revenue":  true,
			"total_sessions": true,
			"a":              true,
			"b":              true,
			"c":              true,
			"engagement":     true,
			"qoe_score":      true,
		},
	}
}

func TestCompile_Golden(t *testing.T) {
	cases := []struct {
		name, src, metricID string
	}{
		{"mean_simple", "mean(heartbeat.value)", "m_mean_simple"},
		{"mean_filtered", "mean(heartbeat.value) where properties.platform = 'mobile'", "m_mean_filtered"},
		{"count_windowed", "count(stream_start) within 7 days of exposure", "m_count_windowed"},
		{"proportion_simple", "proportion(stream_start)", "m_proportion"},
		{"sum_simple", "sum(purchase.amount)", "m_sum_simple"},
		{"percentile_simple", "percentile(95)(latency.value)", "m_percentile"},
		{"count_distinct_simple", "count_distinct(purchase.product_id)", "m_count_distinct"},
		{"composite_two_refs", "0.7 * @watch_time + 0.3 * @ctr", "m_composite_two"},
		{"composite_with_division", "@watch_time / @total_sessions", "m_composite_div"},
		{"composite_with_negate", "-@watch_time + @ctr", "m_composite_negate"},
		{"ratio_top_level", "ratio(@total_revenue, @total_sessions)", "m_ratio"},
		{"composite_with_ratio", "0.5 * @a + 0.5 * ratio(@b, @c)", "m_composite_with_ratio"},
		{"filter_in_list", "mean(heartbeat.value) where properties.platform in ['mobile', 'web']", "m_filter_in"},
		{"filter_multi", "sum(purchase.amount) where country = 'us' and tier != 'free'", "m_filter_multi"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, _, err := Compile(tc.src, testContext(tc.metricID))
			if err != nil {
				t.Fatalf("compile %q: %v", tc.src, err)
			}
			goldenPath := filepath.Join("testdata", tc.name+".golden.sql")
			if *updateGolden {
				if err := os.WriteFile(goldenPath, []byte(got), 0o644); err != nil {
					t.Fatalf("write golden: %v", err)
				}
				return
			}
			want, err := os.ReadFile(goldenPath)
			if err != nil {
				t.Fatalf("read golden %s (re-run with -update to generate): %v", goldenPath, err)
			}
			if got != string(want) {
				t.Errorf("SQL mismatch (re-run with -update to refresh):\n=== GOT ===\n%s\n=== WANT ===\n%s", got, want)
			}
		})
	}
}

// TestCompile_ReturnsRefs validates the dependency-extraction side of Compile.
func TestCompile_ReturnsRefs(t *testing.T) {
	cases := []struct {
		src      string
		wantRefs []string
	}{
		{"mean(x.v)", []string{}},
		{"@a + @b", []string{"a", "b"}},
		{"@a + @b + @a", []string{"a", "b"}}, // deduped
		{"ratio(@num, @den)", []string{"den", "num"}},
		{"0.5 * @a + 0.5 * ratio(@b, @c)", []string{"a", "b", "c"}},
		{"-@watch_time + @ctr", []string{"ctr", "watch_time"}},
	}
	ctx := testContext("m_test")
	ctx.KnownMetricIDs["x"] = true
	ctx.KnownMetricIDs["num"] = true
	ctx.KnownMetricIDs["den"] = true
	for _, tc := range cases {
		t.Run(tc.src, func(t *testing.T) {
			_, refs, err := Compile(tc.src, ctx)
			if err != nil {
				t.Fatalf("compile: %v", err)
			}
			if len(refs) != len(tc.wantRefs) {
				t.Fatalf("refs len: got %d %v, want %d %v", len(refs), refs, len(tc.wantRefs), tc.wantRefs)
			}
			for i, r := range refs {
				if r != tc.wantRefs[i] {
					t.Errorf("refs[%d]: got %q, want %q", i, r, tc.wantRefs[i])
				}
			}
		})
	}
}

// TestCompile_RejectsBareTopLevel ensures defense-in-depth fires if the
// analyzer is somehow bypassed.
func TestCompile_RejectsBareTopLevel(t *testing.T) {
	// We bypass Analyze() by lowering directly.
	ctx := testContext("m_bad")
	mr := &MetricRef{ID: "watch_time"}
	if _, err := lower(mr, ctx); err == nil {
		t.Error("expected lower() to reject bare *MetricRef at top level")
	}
	lit := &Literal{Value: 42}
	if _, err := lower(lit, ctx); err == nil {
		t.Error("expected lower() to reject bare *Literal at top level")
	}
}

// TestCompile_NegateRoundTrip locks in the lowerNegate emission shape.
// Round-6 fix: Negate emits (-sub), and works at any depth.
func TestCompile_NegateRoundTrip(t *testing.T) {
	cases := []struct {
		src, wantContains string
	}{
		{"-@watch_time + @ctr", "(-m1)"}, // m0=ctr, m1=watch_time (sorted)
		{"@watch_time + -@ctr", "(-m0)"}, // m0=ctr, m1=watch_time -> @ctr is m0
		{"@watch_time * -2.5", "(-2.5)"}, // unary on literal
	}
	ctx := testContext("m_negate")
	for _, tc := range cases {
		t.Run(tc.src, func(t *testing.T) {
			sql, _, err := Compile(tc.src, ctx)
			if err != nil {
				t.Fatal(err)
			}
			if !strings.Contains(sql, tc.wantContains) {
				t.Errorf("SQL does not contain %q:\n%s", tc.wantContains, sql)
			}
		})
	}
}

// TestCompile_DivisionUsesNULLIF ensures division emits NULLIF guard.
func TestCompile_DivisionUsesNULLIF(t *testing.T) {
	sql, _, err := Compile("@watch_time / @total_sessions", testContext("m_div"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(sql, "NULLIF(") {
		t.Errorf("division should emit NULLIF guard:\n%s", sql)
	}
}

// TestCompile_FilterEscapesQuotes verifies SQL injection defense.
func TestCompile_FilterEscapesQuotes(t *testing.T) {
	// The lexer rejects unterminated strings, but a value of `it's` is
	// representable only via a quote-escape policy. Since Lock 1 excludes
	// escape sequences in v1, no source string can contain a literal '.
	// We still test the compiler's escape behavior to lock the contract:
	// any embedded ' is doubled.
	val := "it's"
	rendered, err := renderValue(Value{String: &val})
	if err != nil {
		t.Fatal(err)
	}
	if rendered != "'it''s'" {
		t.Errorf("renderValue: got %q, want %q", rendered, "'it''s'")
	}
}

// TestCompile_PercentileFractionConversion verifies AST 0-100 → SQL 0-1.
func TestCompile_PercentileFractionConversion(t *testing.T) {
	sql, _, err := Compile("percentile(95)(latency.value)", testContext("m_p95"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(sql, "percentile_approx(er.value, 0.95)") {
		t.Errorf("percentile_approx fraction wrong (95 should become 0.95):\n%s", sql)
	}
}
