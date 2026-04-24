# Customer Integration Guide — Authoring Implementation Plan

**Status**: Active
**Owner**: Docs coordinator (this session, handing to waves of specialist agents)
**Scope artifact**: [`docs/guides/customer-integration-outline.md`](./customer-integration-outline.md)
**Target output**: A complete, customer-ready integration guide under `docs/guides/integration/`

---

## 1. Purpose

The outline in `customer-integration-outline.md` defines 20 chapters. This plan describes *how* those chapters get written: which agents, in what order, on what branches, with what quality gates, and how we avoid the classic hazards of parallel documentation authoring (voice drift, conflicting cross-references, divergent SDK skeletons, dead links).

The plan is intentionally staged: sequential work establishes anchors; parallel work fills in chapters that depend on those anchors.

---

## 2. Directory Layout

All chapter files live under a single directory so relative cross-references are stable.

```
docs/guides/integration/
  README.md                              # Entry point; mirrors the outline TOC
  templates/
    sdk-chapter-template.md              # Produced in Wave 1, consumed in Wave 2
    feature-chapter-template.md          # Produced in Wave 1, consumed in Wave 3
  01-introduction.md
  02-core-concepts.md
  03-architecture-overview.md
  04-quickstart.md
  05-auth-and-tenancy.md
  06-sdks/
    01-web.md
    02-ios.md
    03-android.md
    04-go.md
    05-python.md
    06-grpc-direct.md
    07-edge.md
  07-experiments.md
  08-feature-flags.md
  09-event-ingestion.md
  10-metrics.md
  11-analysis-and-results.md
  12-bandits.md
  13-advanced-designs.md
  14-operational-integration.md
  15-governance-security-compliance.md
  16-reference/
    README.md
    error-codes.md
    rate-limits.md
    glossary.md
    proto-stability-policy.md
  17-cookbook/
    README.md
    ab-pricing-page.md
    ramp-risky-feature.md
    ...                                  # one file per recipe
  18-troubleshooting-and-faq.md
  19-release-notes-and-deprecation.md
  20-appendices/
    README.md
    ...
```

One file per agent per wave — eliminates merge conflicts inside waves.

---

## 3. Wave Structure

### Wave 1 — Foundations (sequential, 1 agent)

**Agent**: Technical Writer

**Inputs**:
- `CLAUDE.md` (platform overview, module map, critical rules)
- `docs/guides/customer-integration-outline.md` (the outline)
- `docs/design/design_doc_v7.0.md` (architecture truth)
- `docs/adrs/README.md` (ADR index)
- `proto/experimentation/` (module structure)
- Existing `docs/onboarding/` agent-facing docs (for tone calibration *away* from that tone)

**Deliverables**:
1. `docs/guides/integration/README.md` — TOC mirroring the outline, with navigation
2. `docs/guides/integration/01-introduction.md` — Ch 1 fully written
3. `docs/guides/integration/02-core-concepts.md` — Ch 2 fully written (entities, lifecycle, bucketing, events, guardrails, PII boundaries)
4. `docs/guides/integration/03-architecture-overview.md` — Ch 3 fully written (customer-facing module map, data flow, deployment topologies)
5. `docs/guides/integration/templates/sdk-chapter-template.md` — Shared skeleton for §6.1–6.7, with every section heading, "What you'll learn" block, "Next steps" block, and placeholder guidance
6. `docs/guides/integration/templates/feature-chapter-template.md` — Shared skeleton for §7–13

**Quality gates**:
- No ambiguous claim without an ADR or proto reference
- No SDK-specific content in Ch 1–3 (those are SDK-agnostic)
- Glossary terms introduced in Ch 2 are reused verbatim by later waves
- Templates include mandatory sections: Install, Initialize, Assign, Expose, Flag Eval, Shutdown, Error Handling, Troubleshooting

**Exit criteria**: Wave 1 PR merged to `main`. Without the templates, Wave 2 cannot start without risking drift.

---

### Wave 2 — SDK Chapters (parallel, 7 agents)

**Precondition**: Wave 1 merged. Every Wave 2 agent reads `templates/sdk-chapter-template.md` first.

**Isolation**: each agent runs with `isolation: worktree` to avoid stepping on each other. Each produces one file only.

| § | File | Agent | Source of truth |
| --- | --- | --- | --- |
| 6.1 Web | `06-sdks/01-web.md` | Frontend Developer | `sdks/web/` |
| 6.2 iOS | `06-sdks/02-ios.md` | Mobile App Builder | `sdks/ios/` |
| 6.3 Android | `06-sdks/03-android.md` | Mobile App Builder | `sdks/android/` |
| 6.4 Go | `06-sdks/04-go.md` | Backend Architect | `sdks/server-go/` |
| 6.5 Python | `06-sdks/05-python.md` | Backend Architect | `sdks/server-python/` |
| 6.6 gRPC direct | `06-sdks/06-grpc-direct.md` | Technical Writer | `proto/experimentation/` |
| 6.7 Edge | `06-sdks/07-edge.md` | DevOps Automator | `sdks/web/` + edge runtime notes |

**Quality gates per chapter**:
- Every code block must reference a file in `examples/` (create if missing)
- Every API call maps to a proto RPC — name the RPC
- Fallback / offline behavior documented
- Installation instructions cite the actual package registry path used by the SDK

**Consolidation**: single consolidation PR merges all 7 chapter PRs once each passes CI.

---

### Wave 3 — Feature Chapters (parallel, 7 agents)

**Precondition**: Wave 1 merged. Wave 2 may still be in flight — these chapters do not reference SDK specifics.

| § | File | Agent | Primary module |
| --- | --- | --- | --- |
| 7 Experiments | `07-experiments.md` | Technical Writer | M5 Management |
| 8 Feature Flags | `08-feature-flags.md` | Technical Writer | M7 Flags |
| 9 Event Ingestion | `09-event-ingestion.md` | Data Engineer | M2 Pipeline |
| 10 Metrics | `10-metrics.md` | Data Engineer | M3 Metrics |
| 11 Analysis & Results | `11-analysis-and-results.md` | AI Engineer | M4a Analysis |
| 12 Bandits | `12-bandits.md` | AI Engineer | M4b Bandit |
| 13 Advanced Designs | `13-advanced-designs.md` | AI Engineer | M4a + M4b |

**Quality gates**:
- Every statistical claim cites the golden-file table in `CLAUDE.md` (the method, reference package, and precision)
- Every ADR in Cluster A–E referenced at least once across Ch 11–13
- Section 11.5 (peeking safely) explicitly cross-references ADR-015 AVLM and ADR-018 e-values
- Section 12 explicitly cross-references ADR-016 slate bandits and ADR-017 TC/JIVE

---

### Wave 4 — Ops, Reference, Cookbook, Tail (parallel, ~5 agents)

| § | File(s) | Agent |
| --- | --- | --- |
| 14 Operational Integration | `14-operational-integration.md` | SRE |
| 15 Governance / Security / Compliance | `15-governance-security-compliance.md` | Compliance Auditor |
| 16 Reference | `16-reference/*.md` | Technical Writer |
| 17 Cookbook | `17-cookbook/*.md` (10 recipes) | Developer Advocate |
| 18 Troubleshooting & FAQ | `18-troubleshooting-and-faq.md` | Support Responder |
| 19 Release Notes & Deprecation | `19-release-notes-and-deprecation.md` | Technical Writer |
| 20 Appendices | `20-appendices/*.md` | Technical Writer |

---

### Wave 5 — Editorial Pass (sequential, 1 agent)

**Agent**: Technical Writer

- Read every chapter top-to-bottom in outline order
- Enforce consistent voice, tense, terminology
- Verify every internal link resolves
- Verify every `examples/` reference exists
- Produce `docs/guides/integration/CHANGELOG.md` and update `README.md` TOC with page counts and estimated read time

---

## 4. Branching and PR Strategy

- **Wave 1**: single branch `claude/kaizen-integration-docs-waves` → PR → merge to `main`.
- **Wave 2/3/4**: each agent runs in a worktree off `main` on a branch named `claude/kaizen-docs-w{wave}-{slug}`. Each opens its own draft PR. A coordinator session (future) rebases and merges in order.
- **Wave 5**: single branch `claude/kaizen-docs-editorial`.

No wave modifies files owned by a different wave. The outline file itself is only edited in Wave 5.

---

## 5. Shared Conventions (enforced by templates and reviewed in Wave 5)

- Every chapter begins with:
  ```markdown
  > **What you'll learn**
  > - bullet 1
  > - bullet 2
  > - bullet 3
  ```
- Every chapter ends with a `## Next steps` section that links to the logical next chapter.
- Port numbers, module names, and ADR identifiers use the canonical forms from `CLAUDE.md`.
- Code blocks are fenced with language identifiers and, where possible, a `// docs/examples/<file>` header comment pointing at the example repo path.
- Callouts use GitHub-flavored admonitions:
  ```markdown
  > [!NOTE]
  > [!WARNING]
  > [!IMPORTANT]
  ```
- Stability: tag preview/beta surface with `**Status: Preview**` at the section top.

---

## 6. Quality Gates (applied in every wave's CI)

1. **markdownlint** with the project's config (add if missing — deferred to Wave 1)
2. **Link checker** (lychee) for internal and external links
3. **Spelling** (cspell) with a project dictionary
4. **Code block language tags present** (custom check — deferred)
5. **No TODOs merged to `main`** (grep guard; TODOs allowed only during wave PRs, resolved before merge)

---

## 7. Risks and Mitigations

| Risk | Mitigation |
| --- | --- |
| SDK chapters drift in structure | Wave 1 template, reviewed before Wave 2 |
| Voice drift across agents | Wave 5 editorial pass; shared tone guide in `templates/` |
| Stale proto / SDK references | Every code sample ties to a path in `examples/` with CI validation |
| Over-claiming statistical capability | Ch 11–13 must cite ADR + golden-file source per claim |
| Merge conflicts from parallel agents | One file per agent per wave; worktree isolation in Waves 2–4 |
| Review bottleneck | Module owners assigned at wave kickoff, not at PR time |

---

## 8. Success Criteria

- All 20 chapters of the outline have a corresponding non-empty file under `docs/guides/integration/`
- A new customer can complete §4 Quickstart in under 15 minutes with only the docs and an API key
- Every statistical method mentioned has a golden-file reference
- Zero broken internal links
- The guide passes `markdownlint`, `lychee`, and `cspell` in CI
- Module owners (Agents 1, 2, 3, 4, 5, 7) have each reviewed at least one chapter in their domain

---

## 9. Current Status

- [x] Outline merged (`customer-integration-outline.md`, PR #448)
- [x] Implementation plan drafted (this document)
- [ ] Wave 1 launched
- [ ] Wave 1 merged
- [ ] Wave 2 launched (parallel, 7 agents)
- [ ] Wave 3 launched (parallel, 7 agents)
- [ ] Wave 4 launched (parallel, ~5 agents)
- [ ] Wave 5 editorial pass
- [ ] Guide published as the `integration/` landing section

Status is updated by the coordinating session after each wave completes.
