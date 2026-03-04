# Agent-6 Quickstart: M6 Decision Support UI (TypeScript, UI Only)

## Your Identity

| Field | Value |
|-------|-------|
| Module | M6: Decision Support UI |
| Language | TypeScript (Next.js + React) |
| Directory | `ui/` |
| Proto package | You consume all other modules' APIs via `@connectrpc/connect-web` |
| Infra you own | None — you are a frontend-only application |
| Primary SLA | "View SQL" renders < 200ms, all dashboards interactive < 1s, notebook export < 5s |

## The Bright Line

**You render data. You never compute it.** No statistical calculations, no metric aggregations, no bandit policy evaluation. Every number you display comes from M4a (analysis), M4b (bandit), M3 (query log), or M5 (config). If you find yourself writing a `Math.sqrt()` on experiment data, stop — that computation belongs in a Rust crate.

## Read These First (in order)

1. **Design doc v5.1** — Sections 10 (M6 specification), 10.2 (SVOD-specific views), 2.7 (SQL transparency & notebook export)
2. **Proto files** — All service protos (you call them all): `management_service.proto`, `analysis_service.proto`, `bandit_service.proto`, `metrics_service.proto`, `flags_service.proto`
3. **Mermaid diagrams** — `system_architecture.mermaid` (you're the green box reading from PostgreSQL and M5), `state_machine.mermaid` (you show experiment state indicators)

## Who You Depend On (upstream)

| Module | What you need from them | Blocks you? |
|--------|------------------------|-------------|
| M5 (Agent-5) | Experiment CRUD APIs | **Yes** — your experiment list/detail pages are empty without M5. Mock API responses initially. |
| M4a (Agent-4) | Analysis results (treatment effects, CIs, p-values, sequential boundaries, novelty, interference) | **Yes for results pages** — mock with static JSON initially. |
| M4b (Agent-4) | Bandit arm allocation dashboards, cold-start status | Phase 3 dependency only. |
| M3 (Agent-3) | Query log entries for "View SQL" feature | Phase 1 dependency. Mock with sample SQL strings. |

## Who Depends on You (downstream)

Nobody directly depends on you for their work. You are the end-user-facing layer. But the platform has no value without a usable UI, so your pace determines when stakeholders can see the system working.

## Your First PR: Experiment List + Detail Shell

**Goal**: A Next.js application that lists experiments (from M5 API) and shows a detail page with state indicator and variant configuration.

```
ui/
├── package.json
├── next.config.js
├── src/
│   ├── app/
│   │   ├── layout.tsx
│   │   ├── page.tsx                    # Experiment list
│   │   └── experiments/
│   │       └── [id]/
│   │           ├── page.tsx            # Experiment detail
│   │           ├── results/page.tsx    # Analysis results (Phase 2)
│   │           └── sql/page.tsx        # View SQL (Phase 1)
│   ├── components/
│   │   ├── ExperimentTable.tsx
│   │   ├── StateIndicator.tsx          # Color-coded state badge
│   │   ├── VariantTable.tsx
│   │   └── charts/                     # Recharts wrappers (Phase 2+)
│   ├── lib/
│   │   ├── api.ts                      # ConnectRPC client setup
│   │   └── types.ts                    # Generated from protos
│   └── __tests__/                      # vitest + React Testing Library
```

**Acceptance criteria**:
- Experiment list page shows name, type, state (color-coded), owner, created date.
- State indicator: DRAFT = gray, STARTING = yellow pulse, RUNNING = green, CONCLUDING = orange pulse, CONCLUDED = blue, ARCHIVED = gray italic.
- Experiment detail shows variant table (name, traffic fraction, is_control badge, payload preview).
- STARTING and CONCLUDING states show a progress indicator (not stale results).
- CONCLUDED state shows "Results available" link to results page.

**Why this first**: A visible, working experiment list validates that M5's APIs are functional and gives stakeholders something to interact with. The state indicator demonstrates the transitional state machine in action.

## Phase-by-Phase Deliverables

### Phase 0 (Week 1)
- [ ] Next.js project scaffolded with TypeScript, Tailwind, vitest
- [ ] ConnectRPC client configured against M5 management service
- [ ] Generated types from proto schema (`buf generate`)

### Phase 1 (Weeks 2–7)
- [ ] Experiment list page with filtering by state, type, owner
- [ ] Experiment detail page with variant table
- [ ] State indicator with transitional state animations
- [ ] Create/edit experiment form (all experiment types)
- [ ] Layer visualization: bucket allocation bar chart
- [ ] "View SQL" page: renders all SQL from query_log for an experiment, syntax-highlighted
- [ ] Notebook export button (calls M3 `ExportNotebook`, triggers browser download)
- [ ] Metric definition browser

### Phase 2 (Weeks 6–11)
- [ ] Results dashboard: treatment effects table with significance indicators
- [ ] Confidence interval forest plots (Recharts)
- [ ] Sequential testing boundary plot (mSPRT confidence sequences, GST spending function)
- [ ] SRM warning banner (prominent red banner when SRM detected)
- [ ] CUPED variance reduction indicator (show % reduction, toggle CUPED on/off view)
- [ ] Guardrail status panel with breach history
- [ ] Interleaving win-rate bar chart
- [ ] QoE metric sparklines (TTFF, rebuffer rate, bitrate)

### Phase 3 (Weeks 10–17)
- [ ] Bandit arm allocation dashboard: real-time arm probability pie chart
- [ ] Cold-start bandit monitoring: exploration progress, convergence indicator
- [ ] Surrogate projection panel: projected long-term effect with confidence badge (green/yellow/red by R²)
- [ ] Novelty effect curve: fitted exponential decay with steady-state projection line
- [ ] Interference Venn diagram (content overlap) and Lorenz curve (consumption concentration)
- [ ] Lifecycle forest plot: per-segment treatment effects with Cochran Q heterogeneity indicator
- [ ] Cumulative holdout time-series: total algorithmic lift over time
- [ ] GST boundary visualization: stopping boundaries with current test statistic trajectory

### Phase 4 (Weeks 16–22)
- [ ] Dashboard performance: all views render < 1s with 50 concurrent users
- [ ] "View SQL" renders < 200ms (syntax highlighting must not block main thread)
- [ ] Notebook export < 5s for experiments with 20+ metrics
- [ ] Accessibility audit (WCAG 2.1 AA)

## Local Development

```bash
cd ui
npm install
npm run dev          # http://localhost:3000

# Point to local M5 service
NEXT_PUBLIC_API_URL=http://localhost:50055 npm run dev

# Run tests
npm run test         # vitest
npm run lint         # eslint
npm run type-check   # tsc --noEmit
```

## Testing Expectations

- **Component tests**: vitest + React Testing Library. Every component has tests for: renders correctly, handles loading state, handles error state, handles empty state.
- **State indicator**: Test all 6 states render correct color and animation.
- **API mocking**: Use MSW (Mock Service Worker) to mock ConnectRPC responses. Test the full page lifecycle: loading → data → render.
- **Visual regression**: Optional but recommended — Playwright screenshots for key pages.

## Common Pitfalls

1. **No client-side statistics**: It's tempting to compute a quick confidence interval or p-value in the browser. Don't. If you need a number, it comes from M4a. This is not a performance concern — it's a correctness and consistency concern.
2. **State transition rendering**: When an experiment is in STARTING or CONCLUDING, do NOT show stale results from a previous analysis run. Show a progress indicator. Query results APIs should return 503 during CONCLUDING — handle this gracefully.
3. **Sequential testing visualization**: mSPRT and GST have very different visualizations. mSPRT shows a confidence sequence (always-valid CI over time). GST shows discrete analysis looks with spending function boundaries. Don't conflate them.
4. **Surrogate confidence colors**: Green (R² > 0.7), Yellow (0.5–0.7), Red (< 0.5). These thresholds are defined in the proto comments. Display the R² value alongside the color badge so users understand the confidence level.
5. **View SQL syntax highlighting**: Use a web worker for syntax highlighting on large SQL queries. Highlighting 500+ lines of SQL on the main thread will freeze the UI.
6. **Chart library choice**: Recharts for standard charts (bar, line, forest plot). D3 only for custom visualizations (Venn/Lorenz for interference, boundary plots for GST). Don't mix unnecessarily.
