# Kaizen Experimentation — Customer Integration Guide

> **What you'll learn**
> - What this guide is, who it's for, and how to navigate it
> - Which reading path to pick based on your role
> - The current authoring status of every chapter

This guide is the end-to-end reference for integrating Kaizen Experimentation into an SVOD (or similar) product surface. It takes you from "zero credentials" to "running a live experiment with trusted analysis," using a single vocabulary and a single set of cross-references across twenty chapters.

The guide is being authored in five waves by specialist agents. Chapters 1–3 and both templates are complete as of Wave 1. Later chapters are currently stubs and link to `Planned` below until the owning wave ships them. If you land on a `Planned` entry, use the outline in [`customer-integration-outline.md`](../customer-integration-outline.md) for what the chapter will cover.

> [!NOTE]
> Canonical module names and ports used throughout this guide come from the [`CLAUDE.md`](../../../CLAUDE.md) architecture table. If anything in this guide disagrees with `CLAUDE.md`, treat `CLAUDE.md` as the source of truth and file an issue.

---

## 0.1 Reading paths by role

Pick the path that matches how you'll use Kaizen. Each path lists chapters in recommended order; you can always dip into [Chapter 2 — Core Concepts](02-core-concepts.md) when a term is unfamiliar.

### Product Manager — "I want to ship an experiment"

1. [Chapter 1 — Introduction](01-introduction.md)
2. [Chapter 2 — Core Concepts](02-core-concepts.md)
3. [Chapter 4 — Getting Started (Quickstart)](04-quickstart.md)
4. [Chapter 7 — Creating and Managing Experiments](07-experiments.md)
5. [Chapter 11 — Analysis and Results](11-analysis-and-results.md)
6. [Chapter 17 — Cookbook](17-cookbook/README.md)

### Client Engineer (iOS / Android / Web) — "I need to fetch variants"

1. [Chapter 2 — Core Concepts](02-core-concepts.md)
2. [Chapter 3 — Architecture Overview](03-architecture-overview.md)
3. [Chapter 4 — Getting Started (Quickstart)](04-quickstart.md)
4. [Chapter 5 — Authentication, Authorization, and Tenancy](05-auth-and-tenancy.md)
5. Your SDK chapter: [Web](06-sdks/01-web.md), [iOS](06-sdks/02-ios.md), [Android](06-sdks/03-android.md)
6. [Chapter 8 — Feature Flags](08-feature-flags.md)
7. [Chapter 18 — Troubleshooting and FAQ](18-troubleshooting-and-faq.md)

### Backend Engineer — "I need to emit exposure + metric events"

1. [Chapter 2 — Core Concepts](02-core-concepts.md)
2. [Chapter 3 — Architecture Overview](03-architecture-overview.md)
3. [Chapter 5 — Authentication, Authorization, and Tenancy](05-auth-and-tenancy.md)
4. Your SDK chapter: [Go](06-sdks/04-go.md), [Python](06-sdks/05-python.md), or [gRPC direct](06-sdks/06-grpc-direct.md)
5. [Chapter 9 — Event Ingestion](09-event-ingestion.md)
6. [Chapter 14 — Operational Integration](14-operational-integration.md)

### Data / Analytics Engineer — "I need to pipe my metrics in and read results out"

1. [Chapter 2 — Core Concepts](02-core-concepts.md)
2. [Chapter 9 — Event Ingestion](09-event-ingestion.md)
3. [Chapter 10 — Metrics](10-metrics.md)
4. [Chapter 11 — Analysis and Results](11-analysis-and-results.md)
5. [Chapter 13 — Advanced Experimental Designs](13-advanced-designs.md)
6. [Chapter 16 — Reference](16-reference/README.md)

### Platform / DevOps — "I need to deploy or self-host"

1. [Chapter 3 — Architecture Overview](03-architecture-overview.md)
2. [Chapter 5 — Authentication, Authorization, and Tenancy](05-auth-and-tenancy.md)
3. [Chapter 14 — Operational Integration](14-operational-integration.md)
4. [Chapter 15 — Governance, Security, and Compliance](15-governance-security-compliance.md)
5. [Chapter 19 — Release Notes and Deprecation Policy](19-release-notes-and-deprecation.md)
6. [Chapter 20 — Appendices](20-appendices/README.md)

---

## Table of contents

| # | Chapter | Status |
| --- | --- | --- |
| 0 | [How to use this documentation](#01-reading-paths-by-role) (this page) | `Draft` |
| 1 | [Introduction](01-introduction.md) | `Draft` |
| 2 | [Core Concepts](02-core-concepts.md) | `Draft` |
| 3 | [Architecture Overview](03-architecture-overview.md) | `Draft` |
| 4 | [Getting Started (15-Minute Quickstart)](04-quickstart.md) | `Planned` |
| 5 | [Authentication, Authorization, and Tenancy](05-auth-and-tenancy.md) | `Planned` |
| 6 | SDK Integration Guides | `Planned` |
| 6.1 | [Web SDK (TypeScript / React)](06-sdks/01-web.md) | `Planned` |
| 6.2 | [iOS SDK (Swift)](06-sdks/02-ios.md) | `Planned` |
| 6.3 | [Android SDK (Kotlin)](06-sdks/03-android.md) | `Planned` |
| 6.4 | [Server-side Go SDK](06-sdks/04-go.md) | `Planned` |
| 6.5 | [Server-side Python SDK](06-sdks/05-python.md) | `Planned` |
| 6.6 | [Direct gRPC / Protobuf (no SDK)](06-sdks/06-grpc-direct.md) | `Planned` |
| 6.7 | [Edge / CDN integration](06-sdks/07-edge.md) | `Planned` |
| 7 | [Creating and Managing Experiments](07-experiments.md) | `Planned` |
| 8 | [Feature Flags (M7)](08-feature-flags.md) | `Planned` |
| 9 | [Event Ingestion (M2 Pipeline)](09-event-ingestion.md) | `Planned` |
| 10 | [Metrics (M3)](10-metrics.md) | `Planned` |
| 11 | [Analysis and Results (M4a)](11-analysis-and-results.md) | `Planned` |
| 12 | [Bandits and Adaptive Experiments (M4b)](12-bandits.md) | `Planned` |
| 13 | [Advanced Experimental Designs](13-advanced-designs.md) | `Planned` |
| 14 | [Operational Integration](14-operational-integration.md) | `Planned` |
| 15 | [Governance, Security, and Compliance](15-governance-security-compliance.md) | `Planned` |
| 16 | [Reference](16-reference/README.md) | `Planned` |
| 17 | [Cookbook (Task-Oriented Recipes)](17-cookbook/README.md) | `Planned` |
| 18 | [Troubleshooting and FAQ](18-troubleshooting-and-faq.md) | `Planned` |
| 19 | [Release Notes and Deprecation Policy](19-release-notes-and-deprecation.md) | `Planned` |
| 20 | [Appendices](20-appendices/README.md) | `Planned` |

### Shared templates

| Template | Consumer wave | Purpose |
| --- | --- | --- |
| [`templates/sdk-chapter-template.md`](templates/sdk-chapter-template.md) | Wave 2 | Shared skeleton for Chapter 6.1–6.7 |
| [`templates/feature-chapter-template.md`](templates/feature-chapter-template.md) | Wave 3 | Shared skeleton for Chapter 7–13 |

---

## Next steps

- If this is your first visit, start with [Chapter 1 — Introduction](01-introduction.md).
- If you already know what Kaizen is, jump to [Chapter 2 — Core Concepts](02-core-concepts.md) to lock in vocabulary.
- If you're ready to integrate, skip ahead to [Chapter 4 — Getting Started (Quickstart)](04-quickstart.md) once it's published; until then, follow the Client Engineer or Backend Engineer path above.
