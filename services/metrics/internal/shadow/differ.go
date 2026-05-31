package shadow

// differ.go — per-variant equivalence computation for ADR-026 Phase 3 (#437).
//
// Differ reads output for both the original CUSTOM metric and the shadow
// candidate from delta.metric_summaries (via ValueReader), computes diff_abs /
// diff_rel per (experiment, variant, computation_date) tuple, applies the
// metric-type-specific tolerance, and persists one ResultRow per variant via
// Store.InsertResult.
//
// Tolerance rules (from the plan spec):
//
//	COUNT / PROPORTION  → exact match required: diff_abs == 0
//	all other types     → relative tolerance:   diff_rel ≤ 1e-9
//	                       where diff_rel = diff_abs / max(|orig|, 1)
//
// One-side-missing tuples are written with NULL on the missing side and
// within_tolerance = false — they fail the promotion gate without poisoning
// other variants.
//
// Every row written here has a non-empty VariantID; the B2 stub row
// (VariantID == "") is the dedup marker and lives separately in the results
// table.  EvaluatePromotion filters stubs out; the per-variant rows must not
// collide with the stub's key.

import (
	"context"
	"database/sql"
	"math"
	"strings"

	"github.com/google/uuid"
)

// fpTolerance is the maximum relative difference allowed between the original
// and candidate metric values for non-COUNT/PROPORTION metric types.
// Matches the plan spec at 1e-9.
const fpTolerance = 1e-9

// ValueReader reads per-variant metric values from delta.metric_summaries for
// a given (metricID, experimentID, computationDate) triple.  The returned map
// is keyed by variant_id; absent variants are simply missing from the map.
//
// The computationDate is formatted as "YYYY-MM-DD".
//
// Implementations: MockValueReader (tests), PgVariantReader (integration).
type ValueReader interface {
	Read(ctx context.Context, metricID, experimentID, computationDate string) (map[string]float64, error)
}

// Differ computes per-variant equivalence between an original CUSTOM metric
// and a shadow candidate for a single (experiment, computation_date) tuple.
//
// It is constructed by NewDiffer and invoked from computeOneShadow after the
// shadow candidate has been successfully computed and the dedup stub row has
// been written.
type Differ struct {
	reader ValueReader
	store  Store
}

// NewDiffer returns a Differ backed by the given reader and store.
func NewDiffer(reader ValueReader, store Store) *Differ {
	return &Differ{reader: reader, store: store}
}

// Run compares original and shadow candidate outputs for the given (run,
// experimentID, computationDate) triple and writes one ResultRow per variant
// to the store.
//
// metricType drives tolerance: COUNT/PROPORTION → exact (diff_abs == 0), all
// others → diff_rel ≤ 1e-9.
//
// One-side-missing tuples are written with the absent side's value as
// sql.NullFloat64{Valid: false}, diff fields NULL, within_tolerance = false.
//
// VariantID is always non-empty on rows written by Run; stub rows (VariantID
// == "") are owned exclusively by B2.
func (d *Differ) Run(ctx context.Context, run *Run, experimentID, computationDate, metricType string) error {
	origValues, err := d.reader.Read(ctx, run.OriginalMetricID, experimentID, computationDate)
	if err != nil {
		return err
	}

	// B2 writes the shadow candidate to delta.metric_summaries under the
	// shadow UUID as the metric_id (namespace isolation).
	shadowIDStr := run.ShadowID.String()
	candValues, err := d.reader.Read(ctx, shadowIDStr, experimentID, computationDate)
	if err != nil {
		return err
	}

	// Build the union of variant IDs from both sides.
	variantIDs := make(map[string]struct{})
	for v := range origValues {
		variantIDs[v] = struct{}{}
	}
	for v := range candValues {
		variantIDs[v] = struct{}{}
	}

	for variantID := range variantIDs {
		row := ResultRow{
			ResultID:        uuid.New(),
			ShadowID:        run.ShadowID,
			ExperimentID:    experimentID,
			VariantID:       variantID, // always non-empty — no collision with B2 stub
			ComputationDate: computationDate,
		}

		orig, hasOrig := origValues[variantID]
		cand, hasCand := candValues[variantID]

		switch {
		case hasOrig && hasCand:
			// Both sides present — compute diffs and tolerance.
			diffAbs := math.Abs(orig - cand)
			diffRel := diffAbs / math.Max(math.Abs(orig), 1.0)
			within := tolerate(orig, cand, metricType)

			row.OriginalValue = sql.NullFloat64{Float64: orig, Valid: true}
			row.CandidateValue = sql.NullFloat64{Float64: cand, Valid: true}
			row.DiffAbs = sql.NullFloat64{Float64: diffAbs, Valid: true}
			row.DiffRel = sql.NullFloat64{Float64: diffRel, Valid: true}
			row.WithinTolerance = within

		case hasOrig && !hasCand:
			// Original present, candidate missing.
			row.OriginalValue = sql.NullFloat64{Float64: orig, Valid: true}
			row.CandidateValue = sql.NullFloat64{}
			row.DiffAbs = sql.NullFloat64{}
			row.DiffRel = sql.NullFloat64{}
			row.WithinTolerance = false

		case !hasOrig && hasCand:
			// Candidate present, original missing.
			row.OriginalValue = sql.NullFloat64{}
			row.CandidateValue = sql.NullFloat64{Float64: cand, Valid: true}
			row.DiffAbs = sql.NullFloat64{}
			row.DiffRel = sql.NullFloat64{}
			row.WithinTolerance = false
		}

		if err := d.store.InsertResult(ctx, row); err != nil {
			return err
		}
	}

	return nil
}

// tolerate reports whether the difference between orig and cand is within the
// metric-type-specific tolerance.
//
//   - COUNT / PROPORTION: exact match (diff_abs == 0).  Counts stored as
//     DOUBLE PRECISION in delta.metric_summaries are expected to be exact
//     integer-valued when the computation is correct.
//   - all other types:  diff_rel = diff_abs / max(|orig|, 1) ≤ 1e-9.
//     The max(|orig|, 1) denominator prevents division-by-zero when orig == 0.
func tolerate(orig, cand float64, metricType string) bool {
	diffAbs := math.Abs(orig - cand)
	switch strings.ToUpper(metricType) {
	case "COUNT", "PROPORTION":
		return diffAbs == 0
	default:
		denom := math.Max(math.Abs(orig), 1.0)
		return diffAbs/denom <= fpTolerance
	}
}
