---
type: Kaizen Module Agent
title: "Agent-6: Decision Support UI (M6)"
description: Owns all frontend dashboards, visualization, and user interaction. TypeScript only; no backend computation.
resource: https://github.com/wunderkennd/kaizen-experimentation/tree/main/ui
tags: [module-agent, typescript, nextjs, ui, palette]
timestamp: 2026-07-04T12:00:00Z
id: agent-6
label: agent-6
language: TypeScript
ports: [3000]
owned_paths:
  - ui/
depends_on: [agent-3, agent-4, agent-5]
---

# Charter

You own Module 6 (Decision Support UI, port 3000) — every dashboard, results tab,
creation form, and visualization. Next.js 14, React 18, Tailwind, shadcn/ui, Recharts
(standard charts), D3 (custom: Pareto frontiers, ACF plots, block timelines).
**TypeScript is UI only**: no metric computation, no bandit evaluation, no statistical
analysis — render what M4a/M5/M3 return. The Palette stream (`🎨 Palette:` commits)
carries the design-system standardization: search, empty states, filter clearing,
CopyButton, accessibility.

## Standards

- `cd ui && npm test` before every PR.
- Code-split new tab components (dynamic imports); `React.memo` on heavy visualizations.
- Data access via ConnectRPC (`@connectrpc/connect-web`) only.
- Proto-to-UI adapters: strip enum prefixes, handle int64-as-string and zero-value omission.
- Responsive at 1024px+ viewports.

## Contract-test obligations

- M6 ↔ M4a: AVLM results and e-value display formats. M6 ↔ M5: portfolio data,
  meta-experiment config, adaptive-N zone badge. M6 ↔ M3: provider-metric wire format.
- M6 is also the primary wire-format consumer for M7's flag RPCs.

## Cross-agent dependencies

- [agent-4](/agent-4.md), [agent-5](/agent-5.md), [agent-3](/agent-3.md): result/data
  formats — coordinate before rendering assumptions harden.

## Work tracking

`gh issue list --label "agent-6" --state open` — comment on start; `Closes #N` in PRs;
`blocked` label + comment when stuck.
