/**
 * Tests for MetricqlPreview (B5, ADR-026 Phase 2 #436).
 *
 * Uses the injected `previewFn` prop (mirrors B4's `validateFn` pattern) so
 * tests never touch the real API or network. Fake timers cover the 5s timeout.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import { MetricqlPreview } from './preview';
import type { PreviewMetricDefinitionResponse } from '@/lib/api';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makePreviewFn(response: PreviewMetricDefinitionResponse) {
  return vi.fn().mockResolvedValue(response);
}

function pendingPreviewFn(onAbort?: () => void) {
  return vi.fn().mockImplementation(
    (_args: unknown, opts: { signal: AbortSignal }) =>
      new Promise<PreviewMetricDefinitionResponse>((_, reject) => {
        opts.signal.addEventListener('abort', () => {
          if (onAbort) onAbort();
          reject(opts.signal.reason ?? new DOMException('aborted', 'AbortError'));
        });
      }),
  );
}

function timeoutPreviewFn() {
  // Returns a promise that only rejects when the signal fires with TimeoutError.
  return vi.fn().mockImplementation(
    (_args: unknown, opts: { signal: AbortSignal }) =>
      new Promise<PreviewMetricDefinitionResponse>((_, reject) => {
        opts.signal.addEventListener('abort', () =>
          reject(opts.signal.reason ?? new DOMException('timeout', 'TimeoutError')),
        );
      }),
  );
}

// ---------------------------------------------------------------------------
// Test suite
// ---------------------------------------------------------------------------

describe('MetricqlPreview', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  // ── Initial render ──────────────────────────────────────────────────────────

  test('renders collapsed by default — toggle button present, body absent', () => {
    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(heartbeat.playtime_seconds)"
        previewFn={makePreviewFn({ compiledSql: 'SELECT 1', diagnostics: [] })}
      />,
    );

    expect(screen.getByTestId('metricql-preview-toggle')).toBeInTheDocument();
    expect(screen.queryByTestId('metricql-preview-body')).not.toBeInTheDocument();
  });

  test('toggle button has aria-expanded=false when collapsed', () => {
    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={makePreviewFn({ compiledSql: 'SELECT 1', diagnostics: [] })}
      />,
    );
    expect(screen.getByTestId('metricql-preview-toggle')).toHaveAttribute('aria-expanded', 'false');
  });

  // ── Expand / collapse ───────────────────────────────────────────────────────

  test('expands when toggle clicked — body becomes visible', async () => {
    const previewFn = makePreviewFn({ compiledSql: 'SELECT AVG(x.y)', diagnostics: [] });
    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));
    expect(screen.getByTestId('metricql-preview-body')).toBeInTheDocument();
    expect(screen.getByTestId('metricql-preview-toggle')).toHaveAttribute('aria-expanded', 'true');
  });

  test('collapses again when toggle clicked a second time', async () => {
    const previewFn = makePreviewFn({ compiledSql: 'SELECT AVG(x.y)', diagnostics: [] });
    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));
    expect(screen.getByTestId('metricql-preview-body')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));
    expect(screen.queryByTestId('metricql-preview-body')).not.toBeInTheDocument();
  });

  // ── Success path ────────────────────────────────────────────────────────────

  test('renders SQL on success — CodeMirror container present and contains SQL text', async () => {
    const SQL = 'SELECT AVG(playtime_seconds) FROM heartbeat_events';
    const previewFn = makePreviewFn({ compiledSql: SQL, diagnostics: [] });

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(heartbeat.playtime_seconds)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    await waitFor(() =>
      expect(screen.queryByTestId('metricql-preview-sql')).toBeInTheDocument(),
    );

    // CodeMirror renders the SQL text into its DOM tree inside the container.
    expect(screen.getByTestId('metricql-preview-sql').textContent).toContain('AVG');
  });

  test('calls previewFn with experimentId and metricqlExpression', async () => {
    const previewFn = makePreviewFn({ compiledSql: 'SELECT 1', diagnostics: [] });

    render(
      <MetricqlPreview
        experimentId="exp-42"
        metricqlExpression="count(play.start)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    await waitFor(() => expect(previewFn).toHaveBeenCalledTimes(1));

    expect(previewFn).toHaveBeenCalledWith(
      { experimentId: 'exp-42', metricqlExpression: 'count(play.start)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  // ── Guard conditions ────────────────────────────────────────────────────────

  test('shows "fix errors" placeholder when hasErrors=true', () => {
    const previewFn = vi.fn();

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="bad expr !!!"
        hasErrors
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));
    expect(screen.getByText(/fix errors above/i)).toBeInTheDocument();
    expect(previewFn).not.toHaveBeenCalled();
  });

  test('shows "enter an expression" placeholder when expression is empty', () => {
    const previewFn = vi.fn();

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression=""
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));
    expect(screen.getByText(/enter a metricql expression/i)).toBeInTheDocument();
    expect(previewFn).not.toHaveBeenCalled();
  });

  test('shows "enter an expression" placeholder when expression is whitespace-only', () => {
    const previewFn = vi.fn();

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="   "
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));
    expect(screen.getByText(/enter a metricql expression/i)).toBeInTheDocument();
    expect(previewFn).not.toHaveBeenCalled();
  });

  test('does not fetch when pane is closed', () => {
    const previewFn = vi.fn();

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    // No click — pane stays closed.
    expect(previewFn).not.toHaveBeenCalled();
  });

  // ── Loading state ───────────────────────────────────────────────────────────

  test('shows loading skeleton while fetch is in-flight', async () => {
    const previewFn = pendingPreviewFn();

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    await waitFor(() =>
      expect(screen.queryByTestId('metricql-preview-loading')).toBeInTheDocument(),
    );
  });

  // ── Error state ─────────────────────────────────────────────────────────────

  test('shows retry button on network error', async () => {
    const previewFn = vi.fn().mockRejectedValue(new Error('network error'));

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    await waitFor(() =>
      expect(screen.queryByTestId('metricql-preview-retry')).toBeInTheDocument(),
    );

    expect(screen.getByText(/preview failed/i)).toBeInTheDocument();
    expect(screen.getByText(/network error/i)).toBeInTheDocument();
  });

  test('shows server diagnostic message when compiledSql is empty and diagnostics present', async () => {
    const previewFn = makePreviewFn({
      compiledSql: '',
      diagnostics: [
        { severity: 1, message: 'Unknown metric ref @revenue', span: null },
      ],
    });

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    await waitFor(() =>
      expect(screen.queryByTestId('metricql-preview-retry')).toBeInTheDocument(),
    );

    expect(screen.getByText(/unknown metric ref/i)).toBeInTheDocument();
  });

  // ── Timeout state ───────────────────────────────────────────────────────────

  test('shows timeout state + retry-timeout button after 5 seconds', async () => {
    const previewFn = timeoutPreviewFn();

    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    // Advance past the 5 000ms client timeout.
    await act(async () => {
      vi.advanceTimersByTime(5001);
    });

    await waitFor(() =>
      expect(screen.queryByTestId('metricql-preview-retry-timeout')).toBeInTheDocument(),
    );

    expect(screen.getByText(/timed out/i)).toBeInTheDocument();
  });

  // ── Cancel-on-unmount ───────────────────────────────────────────────────────

  test('aborts in-flight request on unmount', async () => {
    const abortSpy = vi.fn();
    const previewFn = pendingPreviewFn(abortSpy);

    const { unmount } = render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    // Wait for fetch to be in-flight (loading skeleton should appear).
    await waitFor(() =>
      expect(screen.queryByTestId('metricql-preview-loading')).toBeInTheDocument(),
    );

    unmount();
    expect(abortSpy).toHaveBeenCalled();
  });

  // ── Re-fetch on expression change ───────────────────────────────────────────

  test('re-fetches when metricqlExpression changes while pane is open', async () => {
    const previewFn = makePreviewFn({ compiledSql: 'SELECT 1', diagnostics: [] });

    const { rerender } = render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));
    await waitFor(() => expect(previewFn).toHaveBeenCalledTimes(1));

    rerender(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="count(play.start)"
        previewFn={previewFn}
      />,
    );

    await waitFor(() => expect(previewFn).toHaveBeenCalledTimes(2));
    expect(previewFn).toHaveBeenLastCalledWith(
      { experimentId: 'exp-1', metricqlExpression: 'count(play.start)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  // ── Global-scope normalisation (Issue #597) ────────────────────────────────
  //
  // The metric-creation form renders MetricqlPreview with experimentId={null}
  // because a metric is not yet bound to an experiment at creation time. The
  // component accepts `string | null | undefined`, normalises to `''` once at
  // the previewFn call site (mirrors PR #595's diagnostics.ts pattern), and
  // forwards the empty wire signal that M5/M3 treat as global scope.

  test('experimentId={null} normalizes to \'\' on the wire', async () => {
    const previewFn = makePreviewFn({ compiledSql: 'SELECT 1', diagnostics: [] });

    render(
      <MetricqlPreview
        experimentId={null}
        metricqlExpression="count(play.start)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    await waitFor(() => expect(previewFn).toHaveBeenCalledTimes(1));

    expect(previewFn).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: 'count(play.start)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  test('experimentId={undefined} normalizes to \'\' on the wire', async () => {
    const previewFn = makePreviewFn({ compiledSql: 'SELECT 1', diagnostics: [] });

    render(
      <MetricqlPreview
        experimentId={undefined}
        metricqlExpression="count(play.start)"
        previewFn={previewFn}
      />,
    );

    fireEvent.click(screen.getByTestId('metricql-preview-toggle'));

    await waitFor(() => expect(previewFn).toHaveBeenCalledTimes(1));

    expect(previewFn).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: 'count(play.start)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  // ── className passthrough ───────────────────────────────────────────────────

  test('applies className to the outer wrapper', () => {
    render(
      <MetricqlPreview
        experimentId="exp-1"
        metricqlExpression="mean(x.y)"
        className="mt-4 sm:col-span-2"
        previewFn={makePreviewFn({ compiledSql: 'SELECT 1', diagnostics: [] })}
      />,
    );

    const wrapper = screen.getByTestId('metricql-preview-toggle').parentElement!;
    expect(wrapper.className).toContain('mt-4');
    expect(wrapper.className).toContain('sm:col-span-2');
  });
});
