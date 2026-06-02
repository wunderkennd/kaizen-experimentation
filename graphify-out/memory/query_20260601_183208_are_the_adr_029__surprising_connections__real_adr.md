---
type: "query"
date: "2026-06-01T18:32:08.313236+00:00"
question: "Are the ADR-029 'surprising connections' real ADR-code links or graph hallucinations?"
contributor: "graphify"
source_nodes: ["adrs_029_cross_modal_calibration", "adrs_029_joint_calibrator", "adrs_029_nev_scale", "adrs_029_experimentation_calibration_crate", "adrs_027_tost", "reward_composer.rs", "slate.rs", "tost.rs"]
---

# Q: Are the ADR-029 'surprising connections' real ADR-code links or graph hallucinations?

## Answer

Expanded via vocab: [calibration, nev, normalized, expected, modal, cross, joint, composer, slate, platt, isotonic, arm]. The 3 ADR-029 surprising connections (JointCalibrator->reward_composer, NEV->slate, ModalityCalibrator->reward_composer) are FALSE POSITIVES. Verified: experimentation-calibration crate does not exist (only 13 crates in workspace, not 14); zero matches for ModalityCalibrator/JointCalibrator/NEVScale anywhere in crates/ or proto/; reward_composer.rs and slate.rs have ZERO mentions of calibration/NEV/ADR-029. ADR-029 status is genuinely Proposed (2026-05-16). The semantic extractor inferred these edges at 0.85 confidence purely from name composability. Cross-check: ADR-027 was tagged as Status Proposed in the header but #443 MERGED; tost.rs cites ADR-027 §2/§3/§6/§7 explicitly; CLAUDE.md confirms TOST shipped. So ADR-027 ↔ tost.rs is REAL but stale-header masked. ADR-026 similarly has Phase 2 #436 (MERGED) showing 'Proposed' in its status table. Filed #586 to fix ADR-027 and ADR-026 status drift. The graph reliability lesson: Proposed-status ADRs generate confident false-positive code links via name similarity, while Accepted/Implemented ADRs with stale headers generate under-confident true-positive links. Both classes look identical in the surprising-connections report — only a content audit distinguishes them.

## Source Nodes

- adrs_029_cross_modal_calibration
- adrs_029_joint_calibrator
- adrs_029_nev_scale
- adrs_029_experimentation_calibration_crate
- adrs_027_tost
- reward_composer.rs
- slate.rs
- tost.rs