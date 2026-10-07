package jobs

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/org/experimentation-platform/services/metrics/internal/spark"
)

var retentionNow = time.Date(2026, 10, 4, 3, 0, 0, 0, time.UTC)

func TestRetentionPolicy_DeleteSQL(t *testing.T) {
	str := RetentionPolicy{Table: "delta.exposures", PartitionColumn: "date_partition", PartitionIsString: true, TTLDays: 90}
	if got, want := str.DeleteSQL(retentionNow), "DELETE FROM delta.exposures WHERE date_partition < '2026-07-06'"; got != want {
		t.Errorf("string partition:\n got %s\nwant %s", got, want)
	}
	date := RetentionPolicy{Table: "delta.metric_summaries", PartitionColumn: "computation_date", TTLDays: 365}
	if got, want := date.DeleteSQL(retentionNow), "DELETE FROM delta.metric_summaries WHERE computation_date < DATE '2025-10-04'"; got != want {
		t.Errorf("date partition:\n got %s\nwant %s", got, want)
	}
	if got, want := date.VacuumSQL(), "VACUUM delta.metric_summaries"; got != want {
		t.Errorf("vacuum: got %s want %s", got, want)
	}
}

func TestDefaultRetentionPolicies_Valid(t *testing.T) {
	for _, p := range DefaultRetentionPolicies() {
		if err := p.validate(); err != nil {
			t.Errorf("default policy invalid: %v", err)
		}
		if !strings.HasPrefix(p.Table, "delta.") {
			t.Errorf("table %q not fully qualified", p.Table)
		}
	}
}

func TestApplyRetentionOverrides(t *testing.T) {
	got, err := ApplyRetentionOverrides(DefaultRetentionPolicies(), "exposures=120, delta.reward_events=200")
	if err != nil {
		t.Fatal(err)
	}
	ttl := map[string]int{}
	for _, p := range got {
		ttl[p.Table] = p.TTLDays
	}
	if ttl["delta.exposures"] != 120 || ttl["delta.reward_events"] != 200 || ttl["delta.metric_events"] != 90 {
		t.Errorf("unexpected TTLs: %v", ttl)
	}
	// Defaults must not be mutated.
	if DefaultRetentionPolicies()[0].TTLDays != 90 {
		t.Error("defaults mutated")
	}

	for _, bad := range []string{"exposures=3", "nope=90", "exposures", "exposures=abc"} {
		if _, err := ApplyRetentionOverrides(DefaultRetentionPolicies(), bad); err == nil {
			t.Errorf("override %q: expected error", bad)
		}
	}
}

func TestNewRetentionJob_RejectsShortTTL(t *testing.T) {
	_, err := NewRetentionJob(spark.NewMockExecutor(0), []RetentionPolicy{{Table: "delta.exposures", PartitionColumn: "date_partition", TTLDays: 1}})
	if err == nil {
		t.Fatal("expected error for ttl below minimum")
	}
}

func TestRetentionJob_Run(t *testing.T) {
	exec := spark.NewMockExecutor(42)
	job, err := NewRetentionJob(exec, DefaultRetentionPolicies())
	if err != nil {
		t.Fatal(err)
	}
	res, err := job.Run(context.Background(), retentionNow)
	if err != nil {
		t.Fatal(err)
	}
	n := len(DefaultRetentionPolicies())
	if len(res.Tables) != n {
		t.Fatalf("got %d table results, want %d", len(res.Tables), n)
	}
	if len(exec.Calls) != 2*n {
		t.Fatalf("got %d SQL calls, want %d (delete + vacuum per table)", len(exec.Calls), 2*n)
	}
	for i, p := range DefaultRetentionPolicies() {
		if exec.Calls[2*i].SQL != p.DeleteSQL(retentionNow) || exec.Calls[2*i+1].SQL != p.VacuumSQL() {
			t.Errorf("table %s: unexpected SQL order %q, %q", p.Table, exec.Calls[2*i].SQL, exec.Calls[2*i+1].SQL)
		}
		if res.Tables[i].RowsDeleted != 42 {
			t.Errorf("table %s: rows deleted %d, want 42", p.Table, res.Tables[i].RowsDeleted)
		}
	}
}

// failingExecutor fails any statement that mentions failTable.
type failingExecutor struct {
	*spark.MockExecutor
	failTable string
}

func (f *failingExecutor) ExecuteSQL(ctx context.Context, sql string) (*spark.SQLResult, error) {
	if strings.Contains(sql, f.failTable) {
		return nil, errors.New("spark unavailable")
	}
	return f.MockExecutor.ExecuteSQL(ctx, sql)
}

func TestRetentionJob_ContinuesPastTableFailure(t *testing.T) {
	exec := &failingExecutor{MockExecutor: spark.NewMockExecutor(1), failTable: "delta.metric_events"}
	job, err := NewRetentionJob(exec, DefaultRetentionPolicies())
	if err != nil {
		t.Fatal(err)
	}
	res, err := job.Run(context.Background(), retentionNow)
	if err == nil || !strings.Contains(err.Error(), "delta.metric_events") {
		t.Fatalf("expected joined error naming metric_events, got %v", err)
	}
	failed := 0
	for _, tr := range res.Tables {
		if tr.Err != nil {
			failed++
		}
	}
	if failed != 1 {
		t.Errorf("got %d failed tables, want 1", failed)
	}
	// Every other table still ran delete + vacuum.
	if want := 2 * (len(DefaultRetentionPolicies()) - 1); len(exec.Calls) != want {
		t.Errorf("got %d successful SQL calls, want %d", len(exec.Calls), want)
	}
}
