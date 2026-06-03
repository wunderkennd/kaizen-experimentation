/**
 * MetricQL inline diagnostics — CodeMirror 6 linter extension (B4, ADR-026 Phase 2 #436).
 *
 * Three-layer responsiveness (per ADR-026 Phase 2 Round-2 amendments):
 *   1. 500ms debounce  — via CM6 linter `delay` option; avoids per-keystroke RPC.
 *   2. AbortController cancel-in-flight — when the user types again before the
 *      previous response arrives, the stale request is aborted. The latest
 *      character's response always wins, deterministically.
 *   3. 2-second client timeout — long-tail M5 responses don't block editing.
 *      On timeout, stale markers stay until the next successful response.
 *
 * The inner source function (`metricqlLintSource`) is exported for direct
 * testing, following the same export-for-testability pattern B3 used for
 * `metricqlCompletionSource`.
 *
 * Anti-patterns intentionally avoided:
 *   - AbortSignal.timeout() — not used; Safari support is recent; we use
 *     setTimeout + ctl.abort() instead.
 *   - Error toasts — transient network failures are silent + console.warn.
 *   - Automatic retries — abort-on-retype makes retries counterproductive.
 *   - Pending markers — stale markers stay until next successful response.
 */

import { linter, Diagnostic } from '@codemirror/lint';
import { EditorView } from '@codemirror/view';
import { Extension } from '@codemirror/state';
import { validateMetricql, ValidateMetricqlRpcResponse } from '@/lib/api';

export type { ValidateMetricqlRpcResponse };

export interface MetricqlLinterOptions {
  /**
   * Experiment ID passed to the ValidateMetricql RPC.
   *
   * Empty / null / undefined all mean "global scope" — Issue #571 Task 1 taught
   * the M5 ValidateMetricql handler to build the known-metric set from the full
   * metric_definitions table (instead of an experiment's bound metrics) when
   * the request's experiment_id is empty. This lets the metric-creation form,
   * which has no experimentId yet, register the linter and receive live
   * diagnostics as the operator types.
   *
   * Captured at mount; if the experimentId prop changes, the editor is
   * remounted by the parent form (B6) so this capture is always fresh.
   *
   * Normalised to `''` at the RPC boundary so the wire format stays a plain
   * string (matches `ValidateMetricqlRequest.experiment_id: string` in proto).
   */
  experimentId: string | null | undefined;

  /**
   * Injected validate function — defaults to the real API client.
   * Overridden in tests to avoid network calls.
   *
   * Returns the parsed RPC response, or null when the request is aborted
   * (either cancel-in-flight or timeout). The linter treats null as
   * "no update to markers right now".
   */
  validateFn?: (
    args: { experimentId: string; metricqlExpression: string },
    options: { signal: AbortSignal },
  ) => Promise<ValidateMetricqlRpcResponse | null>;

  /**
   * Debounce delay in milliseconds. Default 500.
   * CM6 linter already supports this via its `delay` option.
   */
  debounceMs?: number;

  /**
   * Per-request client timeout in milliseconds. Default 2000.
   * When the request takes longer, the AbortController is fired with a
   * TimeoutError reason; the linter logs a console.warn and returns [].
   */
  timeoutMs?: number;
}

/**
 * Build the linter source function. Exported for direct testing so tests can
 * call it with a fake EditorView (or a plain mock with `.state.doc.toString()`)
 * without needing to unwrap the opaque Extension produced by linter().
 *
 * Note on controller lifecycle: each call to `metricqlLintSource` creates a
 * closure that owns `currentController`. The closure is then passed to CM6's
 * `linter()` which calls it on every doc change after `debounceMs` of idle.
 * The controller is shared across invocations via closure, so aborting the
 * previous in-flight request before starting the next is deterministic.
 */
export function metricqlLintSource(opts: MetricqlLinterOptions) {
  const timeoutMs = opts.timeoutMs ?? 2000;
  const validate = opts.validateFn ?? validateMetricql;
  // Normalise null / undefined to '' at construction time so every RPC call
  // sees the same wire-format string (matches proto). M5 treats '' as the
  // global-scope signal (Issue #571 Task 1).
  const experimentIdWire = opts.experimentId ?? '';

  // Mutable state shared across lint invocations for this editor instance.
  let currentController: AbortController | null = null;

  return async (view: EditorView): Promise<Diagnostic[]> => {
    const source = view.state.doc.toString();

    if (!source.trim()) {
      // Empty expression — no markers needed.
      return [];
    }

    // Abort the previous in-flight request before starting a new one.
    if (currentController) {
      currentController.abort();
    }
    const ctl = new AbortController();
    currentController = ctl;

    // Manual timeout: avoid AbortSignal.timeout() for broader browser compat.
    const timer = setTimeout(() => {
      ctl.abort(new DOMException('timeout', 'TimeoutError'));
    }, timeoutMs);

    try {
      const response = await validate(
        { experimentId: experimentIdWire, metricqlExpression: source },
        { signal: ctl.signal },
      );
      clearTimeout(timer);

      // null response means the validate fn observed an abort (cancel-in-flight
      // or timeout handled by the callee). Don't update markers.
      if (response == null) {
        // Log a warning if this was a timeout (not a cancel-in-flight abort).
        if (ctl.signal.aborted) {
          const reason = ctl.signal.reason;
          if (reason instanceof DOMException && reason.name === 'TimeoutError') {
            // eslint-disable-next-line no-console
            console.warn(
              'metricql live-lint timeout (>%dms); markers unchanged until next response',
              timeoutMs,
            );
          }
        }
        return [];
      }

      // Stale-result guard: if another keystroke arrived while we were waiting,
      // currentController will have been replaced. Drop our now-stale result.
      if (currentController !== ctl) {
        return [];
      }

      // Map proto diagnostics to CM6 Diagnostics.
      return response.diagnostics.map((d): Diagnostic => {
        const from = d.span?.startOffset ?? 0;
        // endOffset must be > from for CM6 to render the underline marker.
        // Fall back to from + 1 if span is missing or zero-length.
        const rawTo = d.span?.endOffset ?? from;
        const to = rawTo > from ? rawTo : Math.min(from + 1, source.length);
        return {
          from,
          to,
          severity: d.severity === 2 ? 'warning' : 'error',
          message: d.message,
        };
      });
    } catch (err: unknown) {
      clearTimeout(timer);

      if (ctl.signal.aborted) {
        // Signal was aborted — either cancel-in-flight (user typed again) or
        // timeout (the callee threw instead of returning null).
        // The null-return path already handles the warning above; this covers
        // the case where the callee propagates the AbortError as a throw.
        const reason = ctl.signal.reason;
        if (reason instanceof DOMException && reason.name === 'TimeoutError') {
          // eslint-disable-next-line no-console
          console.warn(
            'metricql live-lint timeout (>%dms); markers unchanged until next response',
            timeoutMs,
          );
        }
        return [];
      }

      // Real network error (M5 down, no connection, etc.).
      // Silent in the UI — don't block editing. Log for diagnostics.
      // eslint-disable-next-line no-console
      console.warn('metricql live-lint failed:', err);
      return [];
    }
  };
}

/**
 * CodeMirror 6 Extension that adds MetricQL inline diagnostics.
 *
 * Usage (inside the MetricqlEditor mount-once useEffect):
 *
 *   extensions.push(metricqlLinter({ experimentId, debounceMs: 500, timeoutMs: 2000 }));
 *
 * Registered unconditionally — when experimentId is null / undefined / '' the
 * linter forwards an empty experiment_id to ValidateMetricql, which M5 treats
 * as global scope (Issue #571). This means standalone usage (e.g. the metric
 * creation form) still surfaces inline diagnostics.
 *
 * The `delay` option on `linter()` is the debounce — CM6 fires the source
 * function only after `debounceMs` ms of idle, so we don't need a separate
 * setTimeout wrapper in the source itself.
 */
export function metricqlLinter(opts: MetricqlLinterOptions): Extension {
  const debounceMs = opts.debounceMs ?? 500;
  return linter(metricqlLintSource(opts), { delay: debounceMs });
}
