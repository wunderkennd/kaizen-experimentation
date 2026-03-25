# Agent-6: Decision Support UI

You own Module 6 (Decision Support UI) — all frontend dashboards, visualization, and user interaction. No backend computation. TypeScript only.

Language: TypeScript
Framework: Next.js 14, React 18, Tailwind CSS, shadcn/ui, Recharts, D3
Directory: `ui/`
Service port: 3000

## Phase 5 ADR Responsibilities

### New Pages
- **`/portfolio`** (ADR-019): Experiment portfolio dashboard. Win rate, learning rate (EwL) trends, annualized impact cumulative chart, traffic utilization gauge, experiment throughput time series, power distribution histogram, false discovery estimate, optimal alpha recommendation widget. Data from M5 portfolio endpoints.
- **`/portfolio/provider-health`** (ADR-014): Provider-side metrics across all running experiments. Time series: catalog coverage, provider Gini, long-tail impression share. Filter by provider. Data from M3 provider metric endpoints.

### New Results Tabs
- **Provider metrics tab** (ADR-014): Provider-side treatment effects alongside user-side metrics. Experiment-level metrics show bootstrap CIs. Guardrail rendering identical to existing user-side guardrails.
- **Feedback loop analysis tab** (ADR-021): Retraining timeline overlaid on daily treatment effect time series. Pre/post retraining comparison box plots. Contamination fraction bar chart per retraining event. Bias-corrected estimate vs. raw estimate side-by-side. Mitigation recommendation matrix. Shown only when `model_retraining_events` data is available.
- **Switchback results tab** (ADR-022): Block timeline with alternating colored treatment/control bands, washout periods grayed. Block-level outcome time series colored by treatment. ACF carryover diagnostic plot. Randomization test distribution histogram with observed statistic marked.
- **Quasi-experiment results tab** (ADR-023): Treated vs. synthetic control two-line time series with vertical treatment onset, shaded confidence band. Pointwise effect plot with confidence bands. Cumulative effect plot. Donor weight table. Placebo test small-multiple panel. Pre-treatment fit RMSPE diagnostic with warning on poor fit.

### Enhanced Existing Views
- **Create experiment form**: Multi-objective reward configuration widget (ADR-011). LP constraint specification UI (ADR-012). Slate bandit configuration (ADR-016) — num_slots, candidates_per_slot, position_bias_model. Switchback parameters (ADR-022) — block_duration, planned_cycles, washout. Quasi-experiment setup (ADR-023) — treated unit, donors, time windows. Optimal alpha recommendation displayed during creation (ADR-019).
- **Results dashboard**: E-value column alongside p-values (ADR-018). AVLM confidence sequence boundary plot replacing separate mSPRT/CUPED views (ADR-015). Adaptive sample size zone indicator — badge showing favorable/promising/futile, extended timeline visualization (ADR-020).
- **Bandit dashboard**: Multi-objective reward decomposition per arm — stacked bar or parallel coordinates showing per-objective contributions (ADR-011). LP constraint adjustment visualization — before/after probability distributions (ADR-012). Slate allocation heatmap — items × slots (ADR-016). MAD randomization fraction indicator (ADR-018).
- **Meta-experiment results** (ADR-013): Objective comparison table (side-by-side variant configs). Business outcome comparison (treatment effects on non-reward metrics). Ecosystem health comparison (provider fairness per variant). Bandit efficiency per variant (convergence, regret). Pareto frontier visualization for 3+ variants (D3 scatter with dominated region shading).

## Coding Standards
- Run `cd ui && npm test` before creating PR.
- Code-split all new tab components with dynamic imports (existing pattern from Phase 4).
- `React.memo` on heavy visualization components.
- Use Recharts for standard charts, D3 for custom visualizations (Pareto frontier, ACF plot, block timeline).
- ConnectRPC client via `@connectrpc/connect-web` — all data from M4a/M5 endpoints.
- Proto-to-UI type adapters: strip enum prefixes, handle int64-as-string, handle zero-value omission.
- Responsive layout: all new pages must work at 1024px+ viewport.
## Work Tracking
Find your assigned work via GitHub Issues:
```bash
gh issue list --label "agent-6" --state open
gh issue view <number>
```
When starting work, comment on the Issue. When creating a PR, include `Closes #<number>`.
If blocked, add the `blocked` label and comment explaining the blocker.

## Dependencies on Other Agents
- Agent-4 (M4a): AVLM result format, e-value format, switchback result format, SCM result format.
- Agent-5 (M5): Portfolio data endpoints, meta-experiment config format, adaptive N zone classification, learning classification enum.
- Agent-3 (M3): Provider metric data format for provider health dashboard.
- Agent-Proto: All new proto types must compile for TypeScript codegen.

## Contract Tests to Write
- M6 ↔ M4a: AVLM results rendering
- M6 ↔ M4a: E-value display format
- M6 ↔ M5: Portfolio data format
- M6 ↔ M5: Meta-experiment config rendering
- M6 ↔ M5: Adaptive N zone badge rendering
- M6 ↔ M3: Provider metric wire-format
