'use client';

import { useMemo } from 'react';
import { SqlHighlighter } from '@/components/sql-highlighter';
import type { MetricDefinition } from '@/lib/types';

interface MetricFormPreviewProps {
  metric: MetricDefinition;
  /**
   * The marshaller from `api.ts` produces the proto JSON shape the server
   * actually receives. Wire this through so the preview reflects exactly
   * what's sent — no manual JSON construction, no drift risk.
   */
  marshal: (m: MetricDefinition) => Record<string, unknown>;
}

/**
 * Read-only proto-shape JSON preview of the metric the form would submit.
 *
 * Helps operators verify the wire format before submitting (especially the
 * ADR-026 Phase 1 `typeConfig` oneof encoding, which is the new surface).
 * Renders via the existing `<SqlHighlighter>` component in `language="json"`
 * mode (Prism supports JSON tokens out of the box).
 */
export function MetricFormPreview({ metric, marshal }: MetricFormPreviewProps) {
  const protoJson = useMemo(() => {
    try {
      return JSON.stringify(marshal(metric), null, 2);
    } catch (err) {
      return `// preview unavailable: ${err instanceof Error ? err.message : String(err)}`;
    }
  }, [metric, marshal]);

  return (
    <div className="flex flex-col gap-2" data-testid="metric-form-preview">
      <p className="text-xs font-medium text-gray-700">
        Proto JSON preview (what gets sent to M5):
      </p>
      <SqlHighlighter
        sql={protoJson}
        language="json"
        copyLabel="Copy proto JSON preview"
        copySuccessMessage="Proto JSON copied to clipboard"
      />
    </div>
  );
}
