/**
 * MetricQL editor — lazy-load boundary (B2, ADR-026 Phase 2 #436).
 *
 * IMPORTANT: Consumers MUST import from this file (`./metricql`), never directly
 * from `./metricql/editor`.  This is the next/dynamic boundary that keeps the
 * CodeMirror and Lezer bundles out of the initial page load for all users who
 * are not authoring METRICQL metric types.
 *
 * SSR is disabled because:
 *   1. CodeMirror constructs a real DOM-bound EditorView at instantiation time.
 *   2. The generated Lezer parser (metricql.js) is consumed at module import time.
 *   3. Disabling SSR keeps the entire metricql/ chunk out of the SSR bundle.
 *
 * Bundle effect: the CodeMirror + Lezer code loads only when MetricqlEditor is
 * rendered, which happens only when the user selects "METRICQL" as the metric
 * type in the metric creation form (B6).
 */

import dynamic from 'next/dynamic';
import type { MetricqlEditorProps } from './editor';

export const MetricqlEditor = dynamic<MetricqlEditorProps>(
  () => import('./editor').then((m) => m.MetricqlEditor),
  {
    ssr: false,
    loading: () => (
      <div
        className="h-32 animate-pulse rounded bg-gray-100"
        data-testid="metricql-editor-loading"
        aria-label="Loading MetricQL editor"
        role="status"
      />
    ),
  },
);

export type { MetricqlEditorProps };
