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

// I1: TestEvaluatePromotion_RejectsNonContiguousWindow — 7 passing days that span
// 9 calendar days (gap at day 4 and 5) → REJECTED with gap dates in the reason.
func TestEvaluatePromotion_RejectsNonContiguousWindow(t *testing.T) {
	// Build 7 passing rows using dates with a 2-day gap: days 1,2,3 then days 6,7,8,9.
	// Calendar span = 9 days (2026-05-01 through 2026-05-09) but only 7 observed.
	dates := []string{
		"2026-05-01", "2026-05-02", "2026-05-03",
		// gap at 2026-05-04 and 2026-05-05
		"2026-05-06", "2026-05-07", "2026-05-08", "2026-05-09",
	}
	var rows []ResultRow
	for _, d := range dates {
		rows = append(rows, rowPass(d))
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusRejected, status)
	assert.Equal(t, 7, dwt)
	assert.Equal(t, 7, total)
	assert.Contains(t, reason, "not contiguous")
	assert.Contains(t, reason, "2026-05-04")
	assert.Contains(t, reason, "2026-05-05")
}

// I1: TestEvaluatePromotion_AcceptsContiguous8Days — 8 contiguous days all
// passing → APPROVED.
func TestEvaluatePromotion_AcceptsContiguous8Days(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 8; i++ {
		rows = append(rows, rowPass(fmt.Sprintf("2026-05-%02d", i)))
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusApproved, status)
	assert.Equal(t, 8, dwt)
	assert.Equal(t, 8, total)
	assert.Empty(t, reason)
}

// I8: TestEvaluatePromotion_SixDaysAllCleanIsPending — 6 contiguous days all
// passing → StatusPending, reason mentions "1 more day".
func TestEvaluatePromotion_SixDaysAllCleanIsPending(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 6; i++ {
		rows = append(rows, rowPass(fmt.Sprintf("2026-05-%02d", i)))
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusPending, status)
	assert.Equal(t, 6, dwt)
	assert.Equal(t, 6, total)
	assert.Contains(t, reason, "1 more day")
}

// I9: TestEvaluatePromotion_DaysWithinToleranceLessThanTotalIsRejected — 7
// contiguous days, 1 day has a failing tuple → REJECTED.
// Explicit test that daysWithinTolerance == 6, totalDays == 7 triggers rejection
// (guards against a refactor that accidentally drops the == clause).
func TestEvaluatePromotion_DaysWithinToleranceLessThanTotalIsRejected(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 7; i++ {
		date := fmt.Sprintf("2026-05-%02d", i)
		if i == 4 {
			rows = append(rows, rowFail(date))
		} else {
			rows = append(rows, rowPass(date))
		}
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusRejected, status)
	assert.Equal(t, 6, dwt)
	assert.Equal(t, 7, total)
	assert.Contains(t, reason, "2026-05-04")
}

// helper: build a stub row (VariantID == "") as written by B2's computeOneShadow.
// A stub has all numeric fields NULL and WithinTolerance == false; it is a dedup
// marker only and must not contribute to the 7-day promotion gate.
func rowStub(date string) ResultRow {
	return ResultRow{
		ResultID:        uuid.New(),
		ShadowID:        uuid.New(),
		ExperimentID:    "exp1",
		VariantID:       "", // empty string is the stub marker
		ComputationDate: date,
		OriginalValue:   sql.NullFloat64{Valid: false},
		CandidateValue:  sql.NullFloat64{Valid: false},
		WithinTolerance: false,
	}
}

// TestEvaluatePromotion_IgnoresStubRowsForDedupOnly — 7 contiguous days where
// each day has one stub row (VariantID=="") AND one real passing tuple.
// The stubs must be invisible: EvaluatePromotion must return StatusApproved
// because the 7 real rows all pass.
func TestEvaluatePromotion_IgnoresStubRowsForDedupOnly(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 7; i++ {
		date := fmt.Sprintf("2026-05-%02d", i)
		rows = append(rows, rowStub(date))  // B2 dedup marker — must be ignored
		rows = append(rows, rowPass(date))  // B3 real result — must count
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusApproved, status,
		"stub rows must not prevent approval; only real per-variant rows matter")
	assert.Equal(t, 7, dwt)
	assert.Equal(t, 7, total)
	assert.Empty(t, reason)
}

// TestEvaluatePromotion_StubRowsDoNotCountTowardTotalDays — 5 days of stub-only
// rows (no real per-variant rows at all).  All 5 stubs must be discarded,
// leaving 0 totalDays → StatusPending with "7 more days needed".
func TestEvaluatePromotion_StubRowsDoNotCountTowardTotalDays(t *testing.T) {
	var rows []ResultRow
	for i := 1; i <= 5; i++ {
		rows = append(rows, rowStub(fmt.Sprintf("2026-05-%02d", i)))
	}
	status, dwt, total, reason := EvaluatePromotion(rows)
	assert.Equal(t, StatusPending, status,
		"stub-only rows must not count toward totalDays; result must be PENDING")
	assert.Equal(t, 0, dwt,
		"daysWithinTolerance must be 0 when all rows are stubs")
	assert.Equal(t, 0, total,
		"totalDays must be 0 when all rows are stubs")
	assert.Contains(t, reason, "7 more days",
		"reason must indicate the full 7 days are still needed")
}
