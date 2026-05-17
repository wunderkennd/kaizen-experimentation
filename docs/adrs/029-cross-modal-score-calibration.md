# ADR-029: Cross-Modal Score Calibration for Heterogeneous Slates

**Status**: Proposed
**Date**: 2026-05-16
**Deciders**: Agent-4 (M4a/M4b — statistical methods), Personalization service owners
**Cluster**: G — Personalization Orchestration (new cluster)

---

## Context

The personalization integration (see #543, #544, #545) produces slates that mix
content types within a single shelf: long-form video, short-form video, manga
series, manga chapters, commerce SKUs, commerce bundles, and editorial collections.
Each modality has a fundamentally different outcome distribution:

| Modality | Native outcome | Typical range | Tail |
|----------|----------------|---------------|------|
| Long-form video | Minutes watched | 0–180 | Right-skewed |
| Short-form video | Completion rate | [0, 1] | Bimodal (skip vs complete) |
| Manga series | Chapters read in 7d | 0–~50 | Heavy right tail |
| Manga chapter | Completion rate | [0, 1] | Bimodal |
| Commerce SKU | GMV (currency) | $0–$10K | Sparse, heavy-tailed |
| Commerce bundle | GMV + attach rate | $0–$10K + [0, 1] | Compound |

Production rankers emit raw scores in modality-native units. Two failure modes
follow if these are mixed directly:

1. **Slate composition becomes incoherent.** A commerce ranker predicting $5 GMV
   and a video ranker predicting 30 watch-minutes cannot be compared by raw score —
   the slate composer ends up sorting on units that have no shared meaning.
2. **Kaizen's analysis silently misleads.** ADR-011's multi-objective reward composer
   scalarizes per-objective rewards into a single value. If the per-modality
   predicted outcomes (`ModalityPredictions` in the personalization proto) are
   uncalibrated, the scalarization's weights are meaningless — a 0.1 reward from
   video and a 0.1 reward from commerce represent different real-world value.

This problem does not exist in single-modality kaizen experiments and has no
precedent in the existing ADRs. ADR-011 (multi-objective reward) provides the
scalarization machinery once values are comparable but does not address how to
make them comparable.

### Research grounding

Calibrated recommendation is well-studied. The most directly relevant prior art:

- **Platt scaling** (Platt, 1999) — logistic regression mapping raw scores to
  calibrated probabilities. Works well for bounded outputs (CTR, completion rate).
- **Isotonic regression** (Zadrozny & Elkan, 2002) — non-parametric monotonic
  mapping. More flexible than Platt; handles non-sigmoid score distributions.
- **Beta calibration** (Kull, Silva Filho, Flach 2017) — three-parameter family
  generalizing Platt; useful for skewed outputs.
- **Spline calibration** (Lucena, 2018) — for continuous-valued outputs like
  watch-minutes or GMV.
- **Netflix calibrated recommendations** (Steck, 2018 — Calibrated Recommendations,
  RecSys 2018) — distinct from score calibration; addresses *category* calibration
  (slate composition diversity). Out of scope here.

The novel piece is **cross-modal calibration**: not just calibrating each ranker to
its own outcome distribution, but mapping every modality's calibrated output to a
*single shared utility scale* so that values can be summed, compared, and traded
off in slate composition and reward computation.

---

## Decision

Implement a per-modality calibration layer that maps raw ranker scores to a unified
**normalized expected value (NEV)** in `[0, 1]`, where NEV represents predicted
contribution to a user's normalized engagement-equivalent value. Build this in a
new Rust crate (`experimentation-calibration`) consumed by the personalization
service at scoring time and validated by M4a at analysis time.

The crate is owned by Agent-4 (the same agent that owns `experimentation-stats`)
because calibration is a statistical concern, not a personalization concern. The
personalization service is a *consumer* of calibration models; it is not the owner
of calibration methodology.

### 1. Crate Structure

```
crates/experimentation-calibration/
├── src/
│   ├── lib.rs
│   ├── modality.rs         # ModalityCalibrator trait + per-modality impls
│   ├── isotonic.rs         # Isotonic regression (delegated to existing if exists)
│   ├── platt.rs            # Platt scaling
│   ├── spline.rs           # Smoothing-spline for continuous outputs
│   ├── joint.rs            # Cross-modal joint calibration (NEV mapping)
│   ├── persistence.rs      # RocksDB serialization (matches ADR-002 pattern)
│   ├── validation.rs       # Coverage tests, ECE/MCE computation
│   └── proto.rs            # Conversion to/from CalibrationProvenance proto
├── Cargo.toml
└── tests/
    ├── golden/             # Reference outputs vs scikit-learn calibration
    └── proptests/
```

### 2. Public API

```rust
pub trait ModalityCalibrator: Send + Sync {
    /// Map a raw ranker score for a single item to a calibrated probability or
    /// expected outcome in the modality's native units.
    fn calibrate_raw(&self, raw_score: f64, context: &CalibrationContext) -> f64;

    /// Map the modality-native calibrated value to the unified NEV scale [0, 1].
    /// This is the cross-modal step. Implementations are responsible for the
    /// modality-to-NEV mapping (e.g., 30 watch-minutes → 0.45 NEV).
    fn to_nev(&self, native_calibrated: f64, context: &CalibrationContext) -> f64;

    /// Convenience: raw_score → NEV in one call.
    fn calibrate_to_nev(&self, raw_score: f64, context: &CalibrationContext) -> f64 {
        let native = self.calibrate_raw(raw_score, context);
        self.to_nev(native, context)
    }

    fn calibrator_id(&self) -> &str;
    fn calibrator_version(&self) -> &str;
    fn modality(&self) -> ContentTypeKey;
}

pub struct JointCalibrator {
    calibrators: HashMap<ContentTypeKey, Box<dyn ModalityCalibrator>>,
    /// Per-modality scalar weights applied AFTER `to_nev` to encode business
    /// priorities (e.g., commerce gets a 0.7 multiplier to discount its higher
    /// volatility). These are NOT learned — they are policy.
    modality_weights: HashMap<ContentTypeKey, f64>,
}

impl JointCalibrator {
    pub fn calibrate_slate(&self, items: &[RawSlateItem]) -> Vec<CalibratedSlateItem> {
        items.iter().map(|item| {
            // Bind the calibrator + weight once per item. The slate path
            // deliberately bypasses the trait's `calibrate_to_nev` convenience
            // method and inlines the two-step (calibrate_raw → to_nev) so the
            // native_calibrated value can be both surfaced in the output AND
            // reused for to_nev without recomputing — one `calibrate_raw` call
            // per item, not two.
            let calibrator = &self.calibrators[&item.modality];
            let weight = self.modality_weights[&item.modality];

            let native = calibrator.calibrate_raw(item.raw_score, &item.context);
            let nev = calibrator.to_nev(native, &item.context);
            let weighted = nev * weight;
            assert_finite!(weighted);  // mandatory per CLAUDE.md fail-fast rule

            CalibratedSlateItem {
                item_id: item.item_id.clone(),
                modality: item.modality.clone(),
                native_calibrated: native,
                nev,
                weighted_nev: weighted,
            }
        }).collect()
    }
}
```

Every floating-point path uses `assert_finite!()` per CLAUDE.md fail-fast policy.

### 3. Calibrator Training (Out of Scope; Hooked)

Calibration models are trained offline (Spark or Python notebook) against logged
exposures and ground-truth outcomes from M3. Training is **not** in this crate's
scope. The crate provides:

- A `CalibratorBuilder` that ingests fitted parameters (knot positions for splines,
  Beta distribution parameters for Beta calibration, etc.).
- A binary format for shipping fitted calibrators to P8 via RocksDB snapshot
  (matches existing ADR-002 single-thread + crash-only pattern).
- A `validate_against_holdout` function that computes Expected Calibration Error
  (ECE) and Maximum Calibration Error (MCE) and rejects calibrators that exceed
  thresholds (default: ECE > 0.05 → reject).

### 4. Proto Integration

The personalization proto's `CalibrationProvenance` (in
`personalization/v1/ranking.proto`) populates from the calibrator metadata:

```protobuf
message CalibrationProvenance {
  string calibrator_id = 1;
  string calibrator_version = 2;
  string calibration_method = 3;
  // Per-modality method used. Populated when slate is heterogeneous.
  map<string, string> per_modality_method = 4;
  // ECE/MCE at the calibrator's last validation. Lets M4a flag analyses where the
  // calibrator was already known to be miscalibrated.
  double last_known_ece = 5;
  double last_known_mce = 6;
}
```

### 5. Integration with ADR-011 Multi-Objective Reward Composer

ADR-011's `RewardComposer` accepts a vector of per-objective rewards and scalarizes
via a configurable scalarization (linear, Tchebycheff, etc.). Today it has no
opinion on whether the inputs are comparable.

After this ADR, the reward composer's contract becomes:
- Per-objective rewards MUST be in NEV-comparable units before scalarization.
- The composer takes a `JointCalibrator` reference at construction and refuses to
  scalarize rewards whose modalities are not registered with the calibrator.
- A new field `composer_config.require_calibrated = true` enforces this for
  personalization experiments. Default `false` preserves backward compatibility for
  single-modality experiments.

### 6. Integration with M4a Analysis

M4a's slate analysis (IPW-adjusted, LIPS estimator in `slate.rs:532`) gains a
calibration-aware mode:

- When `ExposureEvent.bandit_context_json` includes a `CalibrationProvenance` block,
  M4a uses the recorded `calibrator_id + version` to reproduce the NEV mapping
  offline.
- M4a refuses to compute aggregate treatment effects across modalities unless every
  exposure in the analysis window used the same `calibrator_id` (or a calibrator
  the analyst explicitly mapped via a migration table).
- This prevents the silent-mislead failure mode at the cost of strict version
  discipline.

---

## Consequences

### Benefits

1. **Heterogeneous slates have a coherent ordering criterion.** Slate composers can
   sort by NEV across modalities without unit confusion.
2. **Multi-objective rewards (ADR-011) become trustworthy** for cross-modality
   experiments — the scalarization operates on commensurable quantities.
3. **M4a analyses are reproducible.** The recorded `calibrator_id + version` lets
   any historical exposure be re-scored offline against the same calibration model.
4. **Calibration drift becomes observable.** ECE/MCE are tracked over time; analysts
   see "the calibrator that produced these exposures had ECE 0.08" rather than
   debugging mysterious effect-size shifts.
5. **The personalization service stays modality-agnostic.** P8 ships modality
   rankers; the calibration crate is the unifying layer. Adding a new modality is a
   new calibrator, not a P8 rewrite.

### Trade-offs

1. **Calibrators must be trained and maintained.** New infrastructure: training
   pipeline, validation gates, model registry, deployment cadence. Estimate: one
   full-time owner once steady-state.
2. **Strict version discipline.** Mid-experiment calibrator changes invalidate
   ongoing analyses. Calibrator rollouts must be coordinated with experiment
   lifecycle.
3. **The "unified NEV scale" is a policy choice, not a discovered truth.** The
   `modality_weights` encode business judgment (does $1 GMV count as more or less
   than 1 watch-minute?). This choice will be contested.
4. **Calibration is a single point of failure.** A bad calibrator update silently
   degrades every personalization experiment downstream. Mitigation: shadow
   evaluation (ADR-030) of every calibrator update before promotion.
5. **NEV is a derived metric, not a real-world quantity.** Stakeholders who want
   "minutes watched" or "GMV" still need the native-units view. M4a must surface
   both: native-units treatment effect AND NEV-units treatment effect.

---

## Implementation Details

### Proto Schema

```protobuf
// In personalization/v1/ranking.proto (already drafted; this adds fields)
message CalibrationProvenance {
  string calibrator_id = 1;
  string calibrator_version = 2;
  string calibration_method = 3;
  map<string, string> per_modality_method = 4;
  double last_known_ece = 5;
  double last_known_mce = 6;
}
```

No changes to kaizen common protos. `ExposureEvent.bandit_context_json` carries the
serialized `RankingProvenance` (which includes `CalibrationProvenance`) per #543.

### Crate Layout / Public API

See "Decision" section above for full API. The crate exports one trait
(`ModalityCalibrator`), three concrete calibrators (`PlattCalibrator`,
`IsotonicCalibrator`, `SplineCalibrator`), one composer (`JointCalibrator`), and
the validation utilities.

### Integration

| Module | Integration |
|--------|-------------|
| M3 Metrics | Feeds ground-truth outcomes to the offline training pipeline. No code change. |
| M4a Analysis | Consumes `CalibrationProvenance` from exposures; refuses cross-calibrator aggregation. |
| M4b Bandit | When slate bandits run in Path B (#544), the reward composer uses `JointCalibrator`. |
| M5 Management | Strategy registry (#545) references calibrator IDs as part of strategy config. |
| M7 Flags | Gates calibrator rollouts (e.g., `calibrator_v3_canary`). |
| P8 (new) | Loads calibrators from RocksDB at startup; calls `calibrate_slate` during slate composition. |

---

## Validation

### Unit Tests / Proptest Invariants

For each calibrator:
- **Monotonicity within modality**: `raw_score_a ≥ raw_score_b → calibrated_a ≥ calibrated_b`.
- **NEV bounded**: `0.0 ≤ to_nev(x) ≤ 1.0` for all finite x in domain.
- **Deterministic**: identical inputs produce identical outputs across runs.
- **Round-trip persistence**: `from_bytes(to_bytes(calibrator)) == calibrator`.
- **Joint calibrator handles empty slates**: returns empty vector, no panic.
- **`assert_finite!` triggers on NaN/Inf raw scores**: fail-fast policy enforced.

Proptest cases per CLAUDE.md: 10K cases nightly.

### Golden-File Tests

Reference implementations and precision targets:

| Calibrator | Reference | Precision |
|------------|-----------|-----------|
| `PlattCalibrator` | scikit-learn `CalibratedClassifierCV(method='sigmoid')` | 6 decimal places |
| `IsotonicCalibrator` | scikit-learn `IsotonicRegression` | 6 decimal places |
| `SplineCalibrator` | scipy `UnivariateSpline` | 4 decimal places |
| `JointCalibrator.calibrate_slate` | Reference notebook (committed to `golden/`) | 6 decimal places |
| ECE / MCE | sklearn-style `expected_calibration_error` (re-implementation; cross-checked against `netcal` library) | 6 decimal places |

Golden files live in `crates/experimentation-calibration/tests/golden/`.

### Integration / Contract Tests

- **M4a refuses cross-calibrator aggregation**: integration test creates two
  exposures with different `calibrator_version` values and asserts that M4a's
  aggregate analysis returns a clear error rather than a silent number.
- **Reward composer requires registered modalities**: with
  `require_calibrated = true`, scalarizing rewards from an unknown modality returns
  `Err`, not a default value.
- **Provenance round-trip**: `RankingProvenance` serialized to
  `bandit_context_json` by P8 deserializes losslessly in M4a.

---

## Dependencies

- **ADR-002** (M4b LMAX single-thread persistence): RocksDB snapshot pattern reused
  for calibrator persistence.
- **ADR-011** (Multi-Objective Reward Composition): consumer; reward composer
  contract extends to require calibrated inputs.
- **ADR-021** (Feedback Loop Interference): `ModelRetrainingEvent` already tracks
  model retraining; this ADR adds calibrator retraining as a logged event of the
  same shape.
- **#543** (Personalization event emission): the provenance JSON path through
  `ExposureEvent.bandit_context_json` is the channel by which M4a sees calibration
  metadata.
- **#544** (Slate-bandit composition path): both Path A and Path B benefit from
  calibration; Path B requires it for the reward composer.
- **#545** (Strategy configuration contract): strategy configs reference
  calibrator IDs; the JSON Schema registry validates calibrator references.
- **Enables future ADR**: cross-modality treatment effect transport (using calibrated
  values to project effects from one modality to another).

---

## Rejected Alternatives

| Alternative | Reason Rejected |
|-------------|-----------------|
| **No calibration; raw scores compared directly** | Demonstrably wrong — units don't compose. Production failure mode within weeks of multi-modal rollout. |
| **Per-modality experiments only; never mix modalities in a slate** | Forfeits the core product capability driving SVOD orchestration (heterogeneous discovery). Forces analysts to run N parallel experiments where 1 would do. |
| **Learn the calibration end-to-end in the ranker** | Ties calibration to ranker training cycle; calibration drift becomes a ranker problem. Loses the audit trail (`calibrator_id`) that M4a depends on for reproducibility. |
| **Use a single global ranker that outputs NEV directly** | A unified multi-modal ranker is a large research project. Per-modality rankers + a calibration layer is incremental, well-understood, and lets each modality team move independently. Defer the unified ranker as a future direction. |
| **Calibration in P8, not in a dedicated crate** | Calibration is a statistical method (ADR-cluster B/E). Putting it in P8 mixes statistical and engineering concerns and bypasses Agent-4's review. Crate ownership matches existing pattern. |

---

## References

- Platt, J. (1999). *Probabilistic outputs for support vector machines and
  comparisons to regularized likelihood methods.* Advances in large margin
  classifiers.
- Zadrozny, B. & Elkan, C. (2002). *Transforming classifier scores into accurate
  multiclass probability estimates.* KDD.
- Kull, M., Silva Filho, T. M., Flach, P. (2017). *Beyond sigmoids: How to obtain
  well-calibrated probabilities from binary classifiers with beta calibration.*
  Electronic Journal of Statistics.
- Steck, H. (2018). *Calibrated Recommendations.* RecSys 2018. (Context, not direct
  prior art — addresses category calibration, complementary problem.)
- Niculescu-Mizil, A. & Caruana, R. (2005). *Predicting good probabilities with
  supervised learning.* ICML.
- `netcal` Python library — cross-checked golden-file reference for ECE/MCE.
- `crates/experimentation-bandit/src/reward_composer.rs` — consumer.
- `crates/experimentation-bandit/src/slate.rs:532` — LIPS estimator, downstream
  beneficiary of consistent NEV-scaled rewards.
- ADR-011, ADR-016, ADR-026 — adjacent decisions.
- #543, #544, #545 — companion design discussions.
