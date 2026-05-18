//go:build integration

package status

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

func TestPgWriter_UpsertOverwritesPrior(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		dsn = "postgres://experimentation:localdev@localhost:5432/experimentation?sslmode=disable"
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()

	// Clean slate.
	if _, err := pool.Exec(ctx,
		`DELETE FROM metric_computation_status WHERE experiment_id = 'test_exp_475'`,
	); err != nil {
		t.Fatal(err)
	}

	w := NewPgWriter(pool)

	if err := w.Write(ctx, Entry{
		ExperimentID:    "test_exp_475",
		MetricID:        "m1",
		ComputationDate: "2026-05-17",
		Status:          Failed,
		Reason:          "first try",
		RecordedAt:      time.Now(),
	}); err != nil {
		t.Fatal(err)
	}

	if err := w.Write(ctx, Entry{
		ExperimentID:    "test_exp_475",
		MetricID:        "m1",
		ComputationDate: "2026-05-17",
		Status:          Completed,
		Reason:          "",
		RecordedAt:      time.Now(),
	}); err != nil {
		t.Fatal(err)
	}

	var statusValue, reason string
	err = pool.QueryRow(ctx, `
        SELECT status, COALESCE(reason, '') FROM metric_computation_status
        WHERE experiment_id = 'test_exp_475' AND metric_id = 'm1' AND computation_date = '2026-05-17'
    `).Scan(&statusValue, &reason)
	if err != nil {
		t.Fatal(err)
	}
	if statusValue != "completed" {
		t.Errorf("upsert failed: status = %q want completed", statusValue)
	}
	if reason != "" {
		t.Errorf("upsert failed: reason = %q want empty", reason)
	}
}
