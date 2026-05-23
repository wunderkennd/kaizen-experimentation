> ## 📌 Execution status (${PLAN_DATE})
>
> **Locked plan:** [`${PLAN_PATH}`](../../tree/main/${PLAN_PATH}) — last touched on main at `${PLAN_SHA}`.

>
> **Branch convention:** `agent-N/feat/adr-XXX-<slug>` per CLAUDE.md. PR body must include `Closes #${ISSUE_NUM}`. Never use auto-generated worker names as branches.
>
> **Read the plan's Locks section first** — they are normative. If a Lock seems wrong, BLOCK and surface it via an issue comment rather than drifting. Recent precedent (PR #567): off-plan PRs that override merged Locks get rejected and waste the review investment.
>
> **Workflow:** invoke `superpowers:subagent-driven-development` — implementer → spec-reviewer → code-quality-reviewer per task; no parallel implementers within a single plan.
>
> **Hygiene gates** (per CLAUDE.md):
> - Conventional commits (`feat(crate):`, `test(crate):`, `docs:`, `chore:`), one logical change per commit.
> - `buf lint proto/ && buf breaking proto/ --against .git#branch=main` before any proto commit.
> - Workspace `cargo clippy --workspace --all-features -- -D warnings` (CI-exact) before pushing.
> - **No `git stash`** in shared worktrees (May-2026 `kind-eagle`/`witty-lion` collision incident) — use `git diff > /tmp/patch && git apply`.
> - **No committed `gen/...` files** — they regenerate via `buf generate`.

<!--
This banner is upserted by `just prime-issue <number>`. Re-running the recipe
rewrites the block between the EXEC-BANNER markers without disturbing the
original spec below. To regenerate after a plan revision: `just prime-issue N`.
-->
