# Testing M6 Decision Support UI

## Dev Server Setup

```bash
cd ui
npm install
NEXT_PUBLIC_MOCK_API=true npm run dev
```

- The app runs on Next.js. Port 3000 is often in use; it will auto-pick 3001.
- `NEXT_PUBLIC_MOCK_API=true` enables MSW (Mock Service Worker) browser-based API mocking — no backend needed.
- The MSW handlers are in `ui/src/__mocks__/handlers.ts` and seed data in `ui/src/__mocks__/seed-data.ts`.

## Mock Data Inventory

10 mock experiments with various states:
- **DRAFT**: `cold_start_bandit` (44444444-...), `adaptive_bitrate_v3` (22222222-...)
- **STARTING**: `onboarding_flow_v2` (55555555-...)
- **RUNNING**: `homepage_recs_v2` (11111111-...), `search_ranking_interleave` (33333333-...), `recommendation_holdout_q1` (77777777-...)
- **CONCLUDING**: `thumbnail_selection_v1` (66666666-...)
- **CONCLUDED**: `session_watch_pattern` (aaaaaaaa-...), `retention_nudge_v1` (88888888-...)
- **ARCHIVED**: `legacy_layout_test` (99999999-...)

Experiments with analysis results: `homepage_recs_v2`, `search_ranking_interleave`, `retention_nudge_v1`, `session_watch_pattern`, `recommendation_holdout_q1`, `thumbnail_selection_v1`, `cold_start_bandit`.

## Key Testing Flows

### Experiment List (/) 
- All 10 experiments render as table rows (`ExperimentRow`)
- Search box filters by name, owner, description, AND experiment ID
- State/type dropdowns filter correctly
- Sorting by name, type, state, created date works

### Experiment Detail (/experiments/[id])
- Breadcrumb shows "Experiments / {name}" with clickable link back
- DRAFT experiments show "Start Experiment" button
- Clicking Start → Confirm triggers toast notification and state transition to RUNNING
- RUNNING experiments show "Conclude Experiment" button

### Results Page (/experiments/[id]/results)
- Experiments WITHOUT analysis results show friendly "Analysis in progress" or "No results yet" empty state (NOT a red error banner)
- Experiments WITH results show the Results Dashboard with dynamic tabs
- Tab deep-linking: `?tab=novelty`, `?tab=interference`, etc. activate the correct tab on load
- Clicking tabs updates the URL search params
- Invalid tab params (e.g., `?tab=holdout` on non-holdout experiment) fall back to Overview

### Metrics Page (/metrics)
- Single NavHeader (no duplicate layout)
- 12 mock metric definitions displayed

### New Experiment (/experiments/new)
- 5-step wizard: Basics → Type Config → Variants → Metrics & Guardrails → Review
- On creation, redirects to detail page with toast "Experiment created successfully"
- Breadcrumb shows "Experiments / New Experiment"

## Toast Notifications
- Success toasts appear in bottom-right corner with green styling
- Auto-dismiss after 5 seconds
- Dismissible via × button
- Triggered on: experiment creation, start, conclude, archive

## Running Tests

```bash
cd ui
npm run test -- --run    # Run all tests (470+ tests, 476 total with 6 skipped)
npm run lint             # ESLint
npm run type-check       # TypeScript check (tsc --noEmit)
```

## Known Issues
- Toast auto-dismiss timer may have already elapsed by the time the page finishes transitioning, making it hard to capture in screenshots. The toast IS visible briefly after actions.
- The dev role switcher in the nav (Mock dropdown) allows switching between Viewer/Analyst/Experimenter/Admin roles for RBAC testing.

## Devin Secrets Needed
None — the M6 UI uses MSW mocking and does not require any external API keys or credentials for testing.
