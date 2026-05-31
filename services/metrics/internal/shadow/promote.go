package shadow

import (
	"fmt"
	"sort"
	"strings"
	"time"
)

// EvaluatePromotion examines the accumulated result rows for a shadow run and
// applies the 7-consecutive-calendar-days-within-tolerance gate from ADR-026
// Phase 3.
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
//   - totalDays < 7                                       → StatusPending
//   - daysWithinTolerance >= 7
//     AND daysWithinTolerance == totalDays
//     AND totalDays == dateSpan (contiguous calendar window)  → StatusApproved
//   - daysWithinTolerance == totalDays
//     AND totalDays >= 7
//     AND totalDays != dateSpan                           → StatusRejected (gap)
//   - otherwise                                          → StatusRejected (failures)
//
// Approval requires `totalDays >= 7`, `daysWithinTolerance == totalDays`, AND
// the observed dates form a contiguous calendar window.  A gap means we never
// observed equivalence for those gap days, so APPROVED is unsafe (the gate
// exists specifically to catch day-of-week / weekly seasonality effects).
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

	// Compute the calendar span: max(date) - min(date) + 1 in calendar days.
	// An empty set is treated as span 0.
	var allDates []string
	for d := range byDate {
		allDates = append(allDates, d)
	}
	sort.Strings(allDates)

	dateSpan := 0
	var gapDates []string
	if len(allDates) > 0 {
		minDate, _ := time.Parse("2006-01-02", allDates[0])
		maxDate, _ := time.Parse("2006-01-02", allDates[len(allDates)-1])
		dateSpan = int(maxDate.Sub(minDate).Hours()/24) + 1

		// Find any dates in the [min,max] range that are absent from the result set.
		present := make(map[string]struct{}, len(allDates))
		for _, d := range allDates {
			present[d] = struct{}{}
		}
		for cursor := minDate; !cursor.After(maxDate); cursor = cursor.AddDate(0, 0, 1) {
			key := cursor.Format("2006-01-02")
			if _, ok := present[key]; !ok {
				gapDates = append(gapDates, key)
			}
		}
	}

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

	// All days pass tolerance AND the window is contiguous — APPROVED.
	if daysWithinTolerance >= 7 && daysWithinTolerance == totalDays && totalDays == dateSpan {
		return StatusApproved, daysWithinTolerance, totalDays, ""
	}

	// All days pass tolerance but the window has gaps — REJECTED (gap).
	if daysWithinTolerance == totalDays && totalDays >= 7 && totalDays != dateSpan {
		var sb strings.Builder
		sb.WriteString(fmt.Sprintf(
			"shadow run window is not contiguous: observed %d days spanning %d calendar days",
			totalDays, dateSpan,
		))
		if len(gapDates) > 0 {
			sb.WriteString(fmt.Sprintf(" (gaps at %s)", strings.Join(gapDates, ", ")))
		}
		return StatusRejected, daysWithinTolerance, totalDays, sb.String()
	}

	// Build a descriptive rejection reason for tolerance failures.
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
