'use client';

/**
 * MetricQL compiled-SQL preview pane (B5, ADR-026 Phase 2 #436).
 *
 * Collapsible panel that calls M5's PreviewMetricDefinition RPC (C2 proxy →
 * M3 CompileMetricqlPreview) and renders the returned Spark SQL with
 * CodeMirror 6 + @codemirror/lang-sql syntax highlighting (read-only).
 *
 * Fetch lifecycle:
 *   - Only triggers when the pane is OPEN AND the expression is non-empty AND
 *     there are no parse/semantic errors (hasErrors prop from B4 linter).
 *   - 5-second client timeout via AbortController + setTimeout.
 *   - Cancels in-flight request on component unmount (cancel-on-unmount).
 *   - Cancels stale request before issuing a new one when deps change.
 *
 * Anti-patterns intentionally avoided (per task spec):
 *   - No eager fetch on mount — only when pane is open.
 *   - No per-keystroke refetch — parent passes expression after its own debounce.
 *   - No silent retry loop — explicit Retry button only.
 *   - No <textarea> — CodeMirror with lang-sql for proper syntax highlighting.
 */

import { useEffect, useRef, useState } from 'react';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { sql } from '@codemirror/lang-sql';
import { defaultHighlightStyle, syntaxHighlighting } from '@codemirror/language';

import { previewMetricDefinition } from '@/lib/api';

export interface MetricqlPreviewProps {
  /**
   * Experiment ID forwarded to M5's PreviewMetricDefinition RPC (via the C2 proxy).
   *
   * Empty / null / undefined all mean "global scope" — Issue #597 taught the M5
   * and M3 preview handlers to accept an empty experiment_id and build the
   * known-metric set from the global metric catalog. This lets the metric
   * creation form (which has no experiment binding yet) preview compiled SQL.
   *
   * Normalised to `''` at the RPC call site so the wire format stays a plain
   * string (matches `PreviewMetricDefinitionRequest.experiment_id: string` in
   * proto3). Mirrors the diagnostics.ts pattern (PR #595).
   */
  experimentId: string | null | undefined;
  metricqlExpression: string;
  /**
   * Whether the expression currently has parse/semantic errors (from B4 linter).
   * When true, the preview fetch is skipped and a placeholder is shown instead.
   */
  hasErrors?: boolean;
  /** Optional Tailwind class for layout — passed through from the form shell (B6). */
  className?: string;
  /**
   * Injected preview function — defaults to the real API client.
   * Overridden in tests to avoid network calls (mirrors B4's validateFn pattern).
   */
  previewFn?: typeof previewMetricDefinition;
}

type PreviewStatus = 'idle' | 'loading' | 'success' | 'error' | 'timeout';

interface PreviewState {
  status: PreviewStatus;
  sql?: string;
  errorMessage?: string;
}

/** Client-side timeout for the PreviewMetricDefinition RPC (milliseconds). */
const PREVIEW_TIMEOUT_MS = 5000;

export function MetricqlPreview({
  experimentId,
  metricqlExpression,
  hasErrors,
  className,
  previewFn = previewMetricDefinition,
}: MetricqlPreviewProps) {
  const [state, setState] = useState<PreviewState>({ status: 'idle' });
  const [isOpen, setIsOpen] = useState(false);

  // Container div for the read-only CodeMirror SQL view.
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  // AbortController for cancel-on-unmount and cancel-on-rerun.
  const controllerRef = useRef<AbortController | null>(null);

  // ── Fetch effect ────────────────────────────────────────────────────────────
  // Triggers when: pane opens, expression changes, hasErrors changes, or
  // experimentId changes. Guards: pane must be open, no errors, non-empty expr.
  useEffect(() => {
    if (!isOpen) return;

    if (hasErrors) {
      setState({ status: 'idle' });
      return;
    }
    if (!metricqlExpression.trim()) {
      setState({ status: 'idle' });
      return;
    }

    // Cancel any in-flight request before starting a new one.
    if (controllerRef.current) {
      controllerRef.current.abort();
    }
    const ctl = new AbortController();
    controllerRef.current = ctl;

    // Manual 5s timeout — avoids AbortSignal.timeout() for broader compat.
    const timer = setTimeout(() => {
      ctl.abort(new DOMException('timeout', 'TimeoutError'));
    }, PREVIEW_TIMEOUT_MS);

    setState({ status: 'loading' });

    // Normalise null / undefined → '' at the RPC boundary so the wire format
    // stays a plain string (matches proto3). M5/M3 treat '' as global scope
    // (Issue #597). Mirrors diagnostics.ts pattern (PR #595).
    previewFn(
      { experimentId: experimentId ?? '', metricqlExpression },
      { signal: ctl.signal },
    )
      .then((resp) => {
        clearTimeout(timer);

        // Drop stale response if the controller was superseded or unmounted.
        if (ctl.signal.aborted) return;

        if (!resp) {
          // null means the callee observed an abort — treated as cancel.
          return;
        }

        if (resp.compiledSql) {
          setState({ status: 'success', sql: resp.compiledSql });
        } else if (resp.diagnostics && resp.diagnostics.length > 0) {
          // Server-side semantic diagnostics not caught by B4's client-side linter.
          setState({
            status: 'error',
            errorMessage: resp.diagnostics[0].message ?? 'Compile failed',
          });
        } else {
          // Empty SQL with no diagnostics — treat as idle.
          setState({ status: 'idle' });
        }
      })
      .catch((err: unknown) => {
        clearTimeout(timer);

        // If aborted check reason: timeout → timeout state; cancel → no-op.
        if (ctl.signal.aborted) {
          const reason = ctl.signal.reason;
          if (reason instanceof DOMException && reason.name === 'TimeoutError') {
            setState({ status: 'timeout' });
          }
          // else: cancel-in-flight from a superseded request — no UI update.
          return;
        }

        setState({
          status: 'error',
          errorMessage: err instanceof Error ? err.message : 'Preview failed',
        });
      });

    // Cleanup: when deps change or the component unmounts, abort the in-flight
    // request and clear the timeout. Without this, closing the pane (isOpen→false)
    // would leave the fetch + timer running; the .then handler would eventually
    // call setState with stale results, flashing old SQL on the next pane open.
    // Devin PR #570 round-1 finding.
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [isOpen, experimentId, metricqlExpression, hasErrors, previewFn]);

  // ── Cancel-on-unmount ────────────────────────────────────────────────────────
  useEffect(() => {
    return () => {
      if (controllerRef.current) {
        controllerRef.current.abort();
      }
      viewRef.current?.destroy();
      viewRef.current = null;
    };
  }, []);

  // ── CodeMirror SQL view ──────────────────────────────────────────────────────
  // Renders into containerRef whenever compiled SQL is available.
  useEffect(() => {
    // Destroy any existing view first (status change or sql change).
    viewRef.current?.destroy();
    viewRef.current = null;

    if (!containerRef.current || state.status !== 'success' || !state.sql) {
      return;
    }

    const editorState = EditorState.create({
      doc: state.sql,
      extensions: [
        sql(),
        syntaxHighlighting(defaultHighlightStyle),
        EditorView.editable.of(false),
        EditorView.lineWrapping,
      ],
    });
    viewRef.current = new EditorView({
      state: editorState,
      parent: containerRef.current,
    });
  }, [state.sql, state.status]);

  // ── Retry helpers ─────────────────────────────────────────────────────────────
  // Closing + reopening causes the fetch effect to re-run on the next open.
  const retryByReopening = () => {
    setIsOpen(false);
    // Defer the reopen to let the close render + effect cleanup run first.
    setTimeout(() => setIsOpen(true), 0);
  };

  return (
    <div className={className}>
      <button
        type="button"
        onClick={() => setIsOpen((prev) => !prev)}
        className="flex w-full items-center justify-between rounded-md border border-gray-300 px-3 py-2 text-sm font-medium hover:bg-gray-50"
        aria-expanded={isOpen}
        data-testid="metricql-preview-toggle"
      >
        <span>Compiled SQL preview</span>
        <span className="text-gray-500" aria-hidden="true">{isOpen ? '▼' : '▶'}</span>
      </button>

      {isOpen && (
        <div
          className="mt-2 rounded-md border border-gray-200 bg-gray-50 p-3"
          data-testid="metricql-preview-body"
        >
          {state.status === 'idle' && (
            <p className="text-sm text-gray-500">
              {hasErrors
                ? 'Fix errors above to see compiled SQL'
                : metricqlExpression.trim() === ''
                  ? 'Enter a MetricQL expression to see compiled SQL'
                  : 'Compiling...'}
            </p>
          )}

          {state.status === 'loading' && (
            <div
              className="h-20 animate-pulse rounded bg-gray-200"
              data-testid="metricql-preview-loading"
            />
          )}

          {state.status === 'success' && (
            <div
              ref={containerRef}
              data-testid="metricql-preview-sql"
              className="overflow-auto rounded font-mono text-sm"
            />
          )}

          {state.status === 'error' && (
            <div className="text-sm text-red-700">
              <p>Preview failed: {state.errorMessage}</p>
              <button
                type="button"
                onClick={retryByReopening}
                onMouseDown={(e) => e.preventDefault()}
                className="mt-2 rounded bg-red-600 px-2 py-1 text-xs text-white hover:bg-red-700"
                data-testid="metricql-preview-retry"
              >
                Retry
              </button>
            </div>
          )}

          {state.status === 'timeout' && (
            <div className="text-sm text-amber-700">
              <p>Preview timed out (M5 took &gt;{PREVIEW_TIMEOUT_MS / 1000}s)</p>
              <button
                type="button"
                onClick={retryByReopening}
                className="mt-2 rounded bg-amber-600 px-2 py-1 text-xs text-white hover:bg-amber-700"
                data-testid="metricql-preview-retry-timeout"
              >
                Retry
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
