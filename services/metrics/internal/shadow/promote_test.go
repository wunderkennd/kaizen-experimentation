package shadow

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/google/uuid"
	"github.com/stretchr/testify/assert"
)

// helper: build a ResultRow with within_tolerance + both values valid.
func rowPass(date string) ResultRow {
	return ResultRow{
		ResultID:        uuid.New(),
		ShadowID:        uuid.New(),
		ExperimentID:    "exp1",
		VariantID:       "v1",
		ComputationDate: date,
		OriginalValue:   sql.NullFloat64{Float64: 1.0, Valid: true},
		CandidateValue:  sql.NullFloat64{Float64: 1.01, Valid: true},
		WithinTolerance: true,
	}
}

// helper: build a ResultRow that fails tolerance.
func rowFail(date string) ResultRow {
	r := rowPass(date)
	r.WithinTolerance = false
	return r
}

// helper: build a ResultRow where the original side is NULL.
func rowNullOriginal(date string) ResultRow {
	r := rowPass(date)
	r.OriginalValue = sql.NullFloat64{Valid: false}
	r.WithinTolerance = false // B3 sets this to false when either side is NULL
	return r
}

// TestEvaluatePromotion_TooFewDays: fewer than 7 distinct dates → StatusPending.
func TestEvaluatePromotion_TooFewDays(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 3; i++ {
		rows = append(rows, rowPass(fmt.Sprintf("2026-05-%02d", i)))
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusPending, status)
	assert.Equal(t, 3, dwt)
	assert.Equal(t, 3, total)
	assert.Contains(t, reason, "4 more days")
}

// TestEvaluatePromotion_AllPassExactly7Days: 7 passing days → StatusApproved.
func TestEvaluatePromotion_AllPassExactly7Days(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 7; i++ {
		rows = append(rows, rowPass(fmt.Sprintf("2026-05-%02d", i)))
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusApproved, status)
	assert.Equal(t, 7, dwt)
	assert.Equal(t, 7, total)
	assert.Empty(t, reason)
}

// TestEvaluatePromotion_AllPassMoreThan7Days: 10 passing days → StatusApproved.
func TestEvaluatePromotion_AllPassMoreThan7Days(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 10; i++ {
		rows = append(rows, rowPass(fmt.Sprintf("2026-05-%02d", i)))
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusApproved, status)
	assert.Equal(t, 10, dwt)
	assert.Equal(t, 10, total)
	assert.Empty(t, reason)
}

// TestEvaluatePromotion_OneTupleFailedOnOneDay: 1 failing tuple on day 3 of 8 →
// StatusRejected, reason mentions 2026-05-03.
func TestEvaluatePromotion_OneTupleFailedOnOneDay(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 8; i++ {
		date := fmt.Sprintf("2026-05-%02d", i)
		if i == 3 {
			rows = append(rows, rowFail(date))
		} else {
			rows = append(rows, rowPass(date))
		}
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusRejected, status)
	assert.Equal(t, 7, dwt)
	assert.Equal(t, 8, total)
	assert.Contains(t, reason, "1 of 8 days")
	assert.Contains(t, reason, "2026-05-03")
}

// TestEvaluatePromotion_PartialDataOnSomeDays: one day has a NULL-sided tuple →
// that day fails even though WithinTolerance is false (defensive check on Valid).
func TestEvaluatePromotion_PartialDataOnSomeDays(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 8; i++ {
		date := fmt.Sprintf("2026-05-%02d", i)
		if i == 5 {
			rows = append(rows, rowNullOriginal(date))
		} else {
			rows = append(rows, rowPass(date))
		}
	}
	status, _, _, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusRejected, status)
	assert.Contains(t, reason, "2026-05-05")
}

// TestEvaluatePromotion_ZeroRows: empty input → StatusPending with 7 days needed.
func TestEvaluatePromotion_ZeroRows(t *testing.T) {
	status, dwt, total, reason := EvaluatePromotion(nil)
	assert.Equal(t, StatusPending, status)
	assert.Equal(t, 0, dwt)
	assert.Equal(t, 0, total)
	assert.Contains(t, reason, "7 more days")
}
