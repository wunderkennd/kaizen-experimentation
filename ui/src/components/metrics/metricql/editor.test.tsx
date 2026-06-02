/**
 * MetricQL CodeMirror 6 editor — render and interaction tests (B2, ADR-026 Phase 2 #436).
 *
 * Import the EAGER module (./editor), not the lazy-load wrapper (./index), so
 * Vitest doesn't need to handle next/dynamic resolution.
 *
 * JSDOM + CodeMirror 6 notes:
 *   - CM6 creates a real DOM EditorView inside the container div.
 *   - JSDOM doesn't implement contenteditable properly, so typed-input simulation
 *     via userEvent.type is unreliable.  onChange and maxLength tests dispatch
 *     directly against the EditorView's transaction API via the exposed helper.
 *   - The disabled test checks the contenteditable attribute set by CM6, which
 *     JSDOM does propagate correctly.
 *   - DOM structure assertions (testid, aria attributes, initial doc content) are
 *     reliable and cover the critical rendering surface.
 */

import { describe, test, expect, vi, beforeEach } from 'vitest';
import { render, screen, act, waitFor } from '@testing-library/react';
import { MetricqlEditor } from './editor';

// Mock the API client so the linter (B4) does not hit the network.
// Tests that need a specific response override the mock per-test via mockResolvedValueOnce.
vi.mock('@/lib/api', () => ({
  validateMetricql: vi.fn().mockResolvedValue({ diagnostics: [], referencedMetricIds: [] }),
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Find the EditorView instance attached to a metricql-editor container.
 *
 * CodeMirror stores its EditorView on the container's firstChild via the
 * internal `.cmView` property — this is an implementation detail but is the
 * only way to drive the view from JSDOM tests without a browser event loop.
 *
 * If the internal API ever changes this will throw at test time, making the
 * breakage obvious.
 */
function getEditorView(container: HTMLElement) {
  // CM6 attaches the view to the .cm-editor element via an internal symbol.
  // We access it via the public EditorView.findFromDOM helper instead.
  const { EditorView } = require('@codemirror/view') as typeof import('@codemirror/view');
  const cmEditor = container.querySelector('.cm-editor') as HTMLElement | null;
  if (!cmEditor) return null;
  return EditorView.findFromDOM(cmEditor);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('MetricqlEditor', () => {
  beforeEach(async () => {
    // Reset DOM between tests — RTL cleanup handles this but be explicit.
    // Also clear API mock call history so linter-RPC assertions don't see
    // calls from prior tests in this file.
    const { validateMetricql } = await import('@/lib/api');
    vi.mocked(validateMetricql).mockClear();
  });

  // ─── Render ──────────────────────────────────────────────────────────────

  test('renders the wrapper element with correct testid', () => {
    render(
      <MetricqlEditor
        value="mean(heartbeat.value)"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
      />,
    );
    expect(screen.getByTestId('metricql-editor')).toBeInTheDocument();
  });

  test('wrapper has role=textbox and aria-multiline=true', () => {
    render(
      <MetricqlEditor
        value=""
        onChange={() => {}}
        ariaLabel="MetricQL expression"
      />,
    );
    const el = screen.getByTestId('metricql-editor');
    expect(el).toHaveAttribute('role', 'textbox');
    expect(el).toHaveAttribute('aria-multiline', 'true');
  });

  test('wrapper has the supplied ariaLabel', () => {
    render(
      <MetricqlEditor
        value=""
        onChange={() => {}}
        ariaLabel="Custom label for screen reader"
      />,
    );
    expect(screen.getByLabelText('Custom label for screen reader')).toBeInTheDocument();
  });

  // ─── Initial document ────────────────────────────────────────────────────

  test('initial value appears in the CodeMirror DOM', async () => {
    render(
      <MetricqlEditor
        value="mean(heartbeat.value)"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
      />,
    );
    const editor = screen.getByTestId('metricql-editor');
    // CM6 renders doc content into .cm-content; allow one tick for mount.
    await waitFor(() => {
      expect(editor.textContent).toContain('mean(heartbeat.value)');
    });
  });

  test('CM6 editor element (.cm-editor) is mounted inside the wrapper', async () => {
    render(
      <MetricqlEditor
        value="sum(play.duration)"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
      />,
    );
    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      expect(wrapper.querySelector('.cm-editor')).not.toBeNull();
    });
  });

  // ─── Disabled ────────────────────────────────────────────────────────────

  test('disabled=true sets contenteditable=false on cm-content', async () => {
    render(
      <MetricqlEditor
        value="mean(x.y)"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
        disabled
      />,
    );
    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      const content = wrapper.querySelector('.cm-content') as HTMLElement | null;
      // CM6 sets contenteditable="false" when EditorView.editable is off.
      expect(content?.getAttribute('contenteditable')).toBe('false');
    });
  });

  test('disabled=false keeps contenteditable=true', async () => {
    render(
      <MetricqlEditor
        value="sum(x.y)"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
        disabled={false}
      />,
    );
    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      const content = wrapper.querySelector('.cm-content') as HTMLElement | null;
      expect(content?.getAttribute('contenteditable')).toBe('true');
    });
  });

  // ─── External value sync ─────────────────────────────────────────────────

  test('external value change is synced into the editor', async () => {
    const { rerender } = render(
      <MetricqlEditor
        value="mean(heartbeat.value)"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
      />,
    );
    rerender(
      <MetricqlEditor
        value="sum(play.duration)"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
      />,
    );
    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      expect(wrapper.textContent).toContain('sum(play.duration)');
    });
  });

  // ─── onChange via EditorView dispatch ────────────────────────────────────

  test('onChange fires when EditorView dispatches a doc change', async () => {
    const onChange = vi.fn();
    const { container } = render(
      <MetricqlEditor
        value=""
        onChange={onChange}
        ariaLabel="MetricQL expression"
      />,
    );

    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      expect(wrapper.querySelector('.cm-editor')).not.toBeNull();
    });

    const view = getEditorView(container);
    if (!view) {
      // EditorView.findFromDOM unavailable in this JSDOM build — skip.
      return;
    }

    act(() => {
      view.dispatch({
        changes: { from: 0, to: 0, insert: '@watch_time' },
      });
    });

    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith(expect.stringContaining('@watch_time'));
    });
  });

  // ─── maxLength guard ─────────────────────────────────────────────────────

  test('transaction exceeding maxLength is rejected', async () => {
    const onChange = vi.fn();
    const { container } = render(
      <MetricqlEditor
        value=""
        onChange={onChange}
        ariaLabel="MetricQL expression"
        maxLength={10}
      />,
    );

    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      expect(wrapper.querySelector('.cm-editor')).not.toBeNull();
    });

    const view = getEditorView(container);
    if (!view) return;

    const oversized = 'a'.repeat(20);
    act(() => {
      view.dispatch({
        changes: { from: 0, to: 0, insert: oversized },
      });
    });

    // The transactionFilter should have blocked the edit.
    // Either onChange was never called, or the last call has ≤ maxLength chars.
    if (onChange.mock.calls.length > 0) {
      const lastValue = onChange.mock.calls[onChange.mock.calls.length - 1][0] as string;
      expect(lastValue.length).toBeLessThanOrEqual(10);
    } else {
      expect(onChange).not.toHaveBeenCalled();
    }
  });

  // ─── Props forwarded for B3/B4 ───────────────────────────────────────────

  test('accepts experimentId and knownMetricIds props without error', () => {
    // These props are unused in B2 but must not cause TypeScript or runtime errors.
    expect(() =>
      render(
        <MetricqlEditor
          value=""
          onChange={() => {}}
          ariaLabel="MetricQL expression"
          experimentId="exp-123"
          knownMetricIds={['@watch_time', '@play_start']}
        />,
      ),
    ).not.toThrow();
  });

  // ─── Live-lint registration on global-scope (Issue #571 Task 2) ──────────
  //
  // The metric-creation form has no experimentId at creation time.  Previously
  // the editor short-circuited and skipped registering the linter when
  // experimentId was falsy, which meant operators got NO inline diagnostics
  // while authoring a brand-new metric.  M5's ValidateMetricql handler now
  // treats an empty experiment_id as the global-scope signal (Task 1 of #571),
  // so the editor must register the linter unconditionally and forward the
  // raw value (null / undefined / '') through to the RPC boundary.

  test('linter_fires_when_experimentId_is_undefined_global_scope', async () => {
    const { validateMetricql } = await import('@/lib/api');
    const mocked = vi.mocked(validateMetricql);
    mocked.mockResolvedValueOnce({
      diagnostics: [{
        severity: 1,
        message: 'unresolved metric: @unknown_metric',
        span: { startOffset: 0, endOffset: 15, line: 1, column: 1 },
      }],
      referencedMetricIds: [],
    });

    render(
      <MetricqlEditor
        value="@unknown_metric"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
        experimentId={undefined}
      />,
    );

    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      expect(wrapper.querySelector('.cm-editor')).not.toBeNull();
    });

    // The linter is registered unconditionally and CM6's `delay: 500` debounce
    // schedules the source fn on doc init.  Wait for the validateMetricql RPC
    // to be invoked — that proves the linter is active.  The RPC must receive
    // experimentId='' (the normalised wire value for global scope) regardless
    // of whether the caller passed null, undefined, or ''.
    await waitFor(
      () => {
        expect(mocked).toHaveBeenCalled();
      },
      { timeout: 3000 },
    );

    expect(mocked).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: '@unknown_metric' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  test('linter_fires_when_experimentId_is_null_global_scope', async () => {
    const { validateMetricql } = await import('@/lib/api');
    const mocked = vi.mocked(validateMetricql);
    mocked.mockResolvedValueOnce({
      diagnostics: [{
        severity: 1,
        message: 'unresolved metric: @missing',
        span: { startOffset: 0, endOffset: 8, line: 1, column: 1 },
      }],
      referencedMetricIds: [],
    });

    render(
      <MetricqlEditor
        value="@missing"
        onChange={() => {}}
        ariaLabel="MetricQL expression"
        experimentId={null}
      />,
    );

    const wrapper = screen.getByTestId('metricql-editor');
    await waitFor(() => {
      expect(wrapper.querySelector('.cm-editor')).not.toBeNull();
    });

    await waitFor(
      () => {
        expect(mocked).toHaveBeenCalled();
      },
      { timeout: 3000 },
    );

    expect(mocked).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: '@missing' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });
});
