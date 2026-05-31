package shadow

import (
	"fmt"
	"sort"
	"strings"
)

// EvaluatePromotion examines the accumulated result rows for a shadow run and
// applies the 7-consecutive-days-within-tolerance gate from ADR-026 Phase 3.
//
// Grouping rules
// --------------
//   - Rows are grouped by ComputationDate.
//   - A date "passes" when ALL (experiment_id, variant_id) tuples for that date
//     satisfy:
//       1. row.WithinTolerance == true, AND
//       2. row.OriginalValue.Valid == true, AND
//       3. row.CandidateValue.Valid == true
//     (Condition 2 and 3 are a defensive double-check: B3 already sets
//     WithinTolerance = false for NULL-sided tuples, but we guard here anyway.)
//
// Status logic
// ------------
//   - totalDays < 7               → StatusPending,  reason = "shadow run needs N more days"
//   - daysWithinTolerance >= 7
//     AND daysWithinTolerance == totalDays  → StatusApproved,  reason = ""
//   - otherwise                   → StatusRejected, reason lists the failing dates
//
// Returns
// -------
//   status              — lifecycle status to transition to (or remain at)
//   daysWithinTolerance — count of dates where every tuple passed
//   totalDays           — count of distinct dates in the result set
//   reason              — human-readable explanation (empty on APPROVED)
func EvaluatePromotion(rows []ResultRow) (status Status, daysWithinTolerance, totalDays int, reason string) {
	// Group tuples by date.
	type dateTuples struct {
		passed []bool // one entry per (experiment, variant) tuple
	}
	byDate := make(map[string]*dateTuples)
	for _, r := range rows {
		dt, ok := byDate[r.ComputationDate]
		if !ok {
			dt = &dateTuples{}
			byDate[r.ComputationDate] = dt
		}
		// A tuple passes only when all three conditions hold.
		passed := r.WithinTolerance && r.OriginalValue.Valid && r.CandidateValue.Valid
		dt.passed = append(dt.passed, passed)
	}

	totalDays = len(byDate)

	// Collect passing and failing date lists (sorted for deterministic output).
	var passingDates []string
	var failingDates []string
	for date, dt := range byDate {
		allPassed := true
		for _, p := range dt.passed {
			if !p {
				allPassed = false
				break
			}
		}
		if allPassed && len(dt.passed) > 0 {
			passingDates = append(passingDates, date)
		} else {
			failingDates = append(failingDates, date)
		}
	}
	sort.Strings(passingDates)
	sort.Strings(failingDates)

	daysWithinTolerance = len(passingDates)

	// Apply the gate.
	if totalDays < 7 {
		remaining := 7 - totalDays
		days := "day"
		if remaining != 1 {
			days = "days"
		}
		return StatusPending,
			daysWithinTolerance,
			totalDays,
			fmt.Sprintf("shadow run needs %d more %s of data", remaining, days)
	}

	if daysWithinTolerance >= 7 && daysWithinTolerance == totalDays {
		return StatusApproved, daysWithinTolerance, totalDays, ""
	}

	// Build a descriptive rejection reason.
	var sb strings.Builder
	nFail := len(failingDates)
	nObs := totalDays
	sb.WriteString(fmt.Sprintf("%d of %d days had tuples outside tolerance", nFail, nObs))
	if len(failingDates) > 0 {
		sb.WriteString(": ")
		sb.WriteString(strings.Join(failingDates, ", "))
	}
	return StatusRejected, daysWithinTolerance, totalDays, sb.String()
}
