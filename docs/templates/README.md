# Document Templates

Copy-from skeletons for the delivery-lifecycle artifacts
(`docs/guides/delivery-lifecycle.md` is the map: which artifact, when, and
which check guards it). Templates carry OKF-style frontmatter — `type` is the
only required key; unknown keys are preserved; path is identity — same
conventions as `docs/agents/registry/`.

| Template | Produces | Lives at | Stage |
| --- | --- | --- | --- |
| `prd-template.md` | PRD | `docs/prds/YYYY-MM-DD-<slug>.md` | Requirements |
| `rfc-template.md` | RFC (issue body) | Issue titled `RFC-NNN: <title>` | Cross-boundary design |
| `ux-spec-template.md` | UX spec | `docs/superpowers/specs/` | UX design (arrives with H7 PR-4, #699) |
| `../superpowers/templates/locked-plan-template.md` | Locked plan | `docs/superpowers/plans/` | Plan |

ADRs have no template file — imitate `docs/adrs/001`–`030` (the corpus is the
convention) with the `documentation-and-adrs` skill; `scripts/check_docs.py`
(H7 PR-3) lints the required sections.

House rules that apply to every artifact:

- **Decisions carry owner + date** ("owner decision 2026-07-04, #681" is the
  citation form). An undated decision is a suggestion.
- **Locks** shift the burden of justification to the challenger — see the
  lifecycle map for the convention.
- Filenames: `YYYY-MM-DD-<slug>.md` for dated artifacts (PRDs, specs, plans);
  `NNN-<slug>.md` for numbered series (ADRs).
