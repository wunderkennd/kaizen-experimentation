<!--
Feature Chapter Template — Wave 3 agents fill this in, one file per feature area.

Usage:
1. Copy this file to `docs/guides/integration/NN-<slug>.md` (e.g., `07-experiments.md`).
2. Replace `<FEATURE NAME>` and all placeholder text in the HTML comments below.
3. Keep ALL section headings in ALL feature chapters identical so the reading experience is
   uniform across chapters 7–13.
4. Every claim that depends on a specific proto message, RPC, or ADR must cite the source.
   If you cannot verify a name in `proto/experimentation/` or `docs/adrs/`, leave a
   `<!-- TODO(wave-3): verify -->` comment and move on.
5. Every statistical claim MUST cite the golden-file table in `CLAUDE.md` — the method, the
   reference package, and the precision. Do not write statistical prose without a citation.
6. Do NOT remove the "What you'll learn" block at the top or the "Next steps" block at the bottom.
-->

# N. <FEATURE NAME>

> **What you'll learn**
> - <bullet 1 — the concept the reader will understand after this chapter>
> - <bullet 2 — the task the reader will be able to perform>
> - <bullet 3 — the pitfall the reader will know how to avoid>

## What You'll Learn

<!--
What goes here: 2–3 paragraphs expanding the bullets above. Frame the chapter in terms of
the customer's job: what problem they're trying to solve, and what they'll walk away with.
Avoid marketing voice — this is a developer talking to another developer.
-->

## Concept

<!--
What goes here: a concrete, self-contained explanation of the feature. Define every new term,
but prefer linking to Chapter 2 §2.x over redefining things. If the concept has a model
(state machine, data flow, lifecycle), include a mermaid diagram.
-->

## When to Use

<!--
What goes here: a decision rubric for when this feature is the right tool. Include at least
one "when NOT to use" paragraph so readers can rule it out quickly. If the feature overlaps
with another chapter's feature, disambiguate explicitly.
-->

## API Surface

<!--
What goes here: a table of the gRPC RPCs relevant to this feature, with the canonical module
name, the port, the request and response proto messages, and a one-line description of what
each RPC does. Example row:

| RPC | Module | Port | Request | Response | Purpose |
| --- | --- | --- | --- | --- | --- |
| `CreateExperiment` | M5 Management | 50055 | `CreateExperimentRequest` | `Experiment` | Create a new experiment in DRAFT state |

Use the canonical module names and port numbers from CLAUDE.md. Verify every RPC name in
`proto/experimentation/<module>/v1/*.proto` — do not invent RPCs.
-->

## Step-by-Step Walkthrough

<!--
What goes here: a numbered walkthrough of the most common end-to-end workflow. Each step:
a short "why," a code block (fenced with language identifier and a `// examples/<path>`
header), and the expected outcome. The walkthrough must run end-to-end against a real
Kaizen instance — no pseudocode, no handwaving.
-->

## Proto Reference

<!--
What goes here: links to every `proto/experimentation/<module>/v1/*.proto` file referenced
above. Include a brief "field map" table for the top-level request/response messages so
readers know what they're filling in. Do NOT paste the entire proto — link to it and
summarize.
-->

## Cross-Module Dependencies

<!--
What goes here: which other modules participate in this feature, what RPC flows cross
module boundaries, and what configuration must be in place in those other modules before
this feature works. Example: "Experiments rely on M2 for exposure ingestion and M3 for
metric computation; if either is unavailable, the experiment will start but not produce
analyzable data." Cross-link to Chapter 3 for the module map.
-->

## Operational Notes

<!--
What goes here: quotas, rate limits, capacity guidance, and failure modes specific to this
feature. Include a "what survives an outage" subsection pointing at Chapter 14. If the
feature has any irreversible operations (salt rotation, archive, delete), call them out
in a `> [!WARNING]` admonition.
-->

## ADR References

<!--
What goes here: a bulleted list of every ADR that governs this feature's design, with a
one-line summary of what each ADR says and what it constrains. Use the format:

- [ADR-NNN](../../adrs/NNN-slug.md) — <one-line summary> (constrains: <what it binds>)

Every ADR referenced here must exist in `docs/adrs/`. If a claim depends on an ADR but the
ADR is still Proposed, say so explicitly.
-->

## Next steps

<!--
Replace these placeholders with real links. Every feature chapter must end with a "Next steps"
block linking to: (a) the logical next chapter in outline order, and (b) at least one
cookbook recipe that exercises the feature.
-->

- Continue to [Chapter TBD](../README.md#table-of-contents) for the next logical step.
- For a runnable recipe that exercises this feature end-to-end, see [Cookbook recipe TBD](../17-cookbook/README.md).
