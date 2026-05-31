// ADR-026 Phase 3 / Task D2 (Lock L5). The M5 server emits the
// `x-kaizen-deprecation` HTTP/2 response header on CUSTOM creates (tonic's
// `Response::metadata_mut()` is initial metadata, not trailers — see
// `crates/experimentation-management/src/grpc.rs` const docstring); the UI
// surfaces it as a toast. Detail-page banner deferred (no metric detail
// page exists yet).
//
// Extracted out of `app/metrics/new/page.tsx` so the helpers can be imported
// from a unit test without tripping Next.js App Router's strict page-export
// allowlist (only `default`, `metadata`, `viewport`, `dynamic`, etc. are
// valid exports from a page module).
//
// The user-visible string is locked by L5 so operator-facing messaging stays
// consistent across M5 (header), the UI toast, and the migration runbook
// referenced at the end. If you change this, also update the runbook anchor
// `docs/runbooks/m5-metric-definitions.md#custom-deprecation`.

import type { MetricType } from '@/lib/types';

export const DEPRECATION_TOAST_MESSAGE =
  'Custom SQL metrics are deprecated. Use MetricQL or structured types instead. See docs/runbooks/m5-metric-definitions.md#custom-deprecation.';

/**
 * ADR-026 Phase 3 / Task D2. Returns true when the just-created metric is
 * a CUSTOM metric and the UI should surface the deprecation toast.
 *
 * The type-gate is unit-testable in isolation — the integration test exercises
 * the full Create → router push → toast emission path via the page, and this
 * helper covers the per-type decision matrix (CUSTOM yes; MEAN, FILTERED_MEAN,
 * METRICQL, etc. no).
 */
export function shouldShowCustomDeprecationToast(metric: { type: MetricType }): boolean {
  return metric.type === 'CUSTOM';
}
