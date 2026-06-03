/**
 * Tests for MetricqlSection (B6, ADR-026 Phase 2 #436).
 *
 * Mocks:
 *   - './metricql' (lazy boundary) → eager MetricqlEditor from './metricql/editor'
 *     so next/dynamic doesn't need browser resolution.
 *   - './metricql/preview' → lightweight stub (the preview pane has its own
 *     suite; here we only verify that it receives the right props).
 *   - '@/lib/api' → { validateMetricql, previewMetricDefinition } stubs so no
 *     network calls are issued by the editor's B4 linter or preview pane.
 */

import { describe, test, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { MetricqlSection } from './metricql-section';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

// Mock the lazy-load boundary so Vitest resolves the real editor eagerly.
// This avoids next/dynamic module resolution issues in jsdom.
vi.mock('./metricql', async () => {
  const mod = await vi.importActual<typeof import('./metricql/editor')>('./metricql/editor');
  return { MetricqlEditor: mod.MetricqlEditor };
});

// Stub the preview pane — its behaviour is fully tested in preview.test.tsx.
// We expose data-testid="metricql-preview-toggle" so B6 section tests can
// assert on its presence without coupling to preview internals.
vi.mock('./metricql/preview', () => ({
  MetricqlPreview: ({ experimentId, metricqlExpression, hasErrors, className }: {
    experimentId: string;
    metricqlExpression: string;
    hasErrors?: boolean;
    className?: string;
  }) => (
    <div
      data-testid="metricql-preview-toggle"
      data-experiment-id={experimentId}
      data-expression={metricqlExpression}
      data-has-errors={String(hasErrors ?? false)}
      className={className}
    />
  ),
}));

// Stub API calls so the linter (B4) and preview pane don't hit the network.
vi.mock('@/lib/api', () => ({
  validateMetricql: vi.fn().mockResolvedValue({ diagnostics: [], referencedMetricIds: [] }),
  previewMetricDefinition: vi.fn().mockResolvedValue({ compiledSql: 'SELECT 1', diagnostics: [] }),
  listMetricDefinitions: vi.fn().mockResolvedValue({ metrics: [], nextPageToken: '' }),
}));

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('MetricqlSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('renders editor and preview toggle', async () => {
    render(
      <MetricqlSection
        value=""
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
      />,
    );

    // Editor present — data-testid from B2
    expect(await screen.findByTestId('metricql-editor')).toBeInTheDocument();
    // Preview toggle stub present
    expect(screen.getByTestId('metricql-preview-toggle')).toBeInTheDocument();
  });

  test('renders inside a METRICQL-labelled fieldset', async () => {
    render(
      <MetricqlSection
        value=""
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');
    const section = screen.getByTestId('metricql-section');
    expect(section.tagName).toBe('FIELDSET');
    expect(section.querySelector('legend')?.textContent).toBe('METRICQL');
  });

  test('passes value through to editor', async () => {
    render(
      <MetricqlSection
        value="mean(heartbeat.value)"
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
      />,
    );

    const editor = await screen.findByTestId('metricql-editor');
    // CodeMirror renders the doc into the DOM tree inside the container div.
    expect(editor.textContent).toContain('mean(heartbeat.value)');
  });

  test('sets hasErrors=true on preview when expression is empty', async () => {
    render(
      <MetricqlSection
        value=""
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');
    const toggle = screen.getByTestId('metricql-preview-toggle');
    expect(toggle.getAttribute('data-has-errors')).toBe('true');
  });

  test('sets hasErrors=false on preview when expression is non-empty', async () => {
    render(
      <MetricqlSection
        value="count(play.start)"
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');
    const toggle = screen.getByTestId('metricql-preview-toggle');
    expect(toggle.getAttribute('data-has-errors')).toBe('false');
  });

  test('passes experimentId through to preview', async () => {
    render(
      <MetricqlSection
        value="mean(x.y)"
        onChange={() => {}}
        experimentId="exp-42"
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');
    const toggle = screen.getByTestId('metricql-preview-toggle');
    expect(toggle.getAttribute('data-experiment-id')).toBe('exp-42');
  });

  test('disabled prop propagates to editor — contenteditable becomes false', async () => {
    render(
      <MetricqlSection
        value="mean(x.y)"
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
        disabled
      />,
    );

    const editor = await screen.findByTestId('metricql-editor');
    const content = editor.querySelector('.cm-content') as HTMLElement | null;
    // CodeMirror sets contenteditable="false" when EditorView.editable is off.
    expect(content?.getAttribute('contenteditable')).toBe('false');
  });

  test('disabled prop also disables the fieldset', async () => {
    render(
      <MetricqlSection
        value=""
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
        disabled
      />,
    );

    await screen.findByTestId('metricql-editor');
    const section = screen.getByTestId('metricql-section');
    expect((section as HTMLFieldSetElement).disabled).toBe(true);
  });

  test('shows helper text mentioning autocomplete', async () => {
    render(
      <MetricqlSection
        value=""
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');
    expect(screen.getByText(/autocomplete/i)).toBeInTheDocument();
  });

  test('shows helper text mentioning @metric_id reference syntax', async () => {
    render(
      <MetricqlSection
        value=""
        onChange={() => {}}
        experimentId="exp-1"
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');
    // The helper text includes "@metric_id" in a <code> element.
    expect(screen.getByText('@metric_id')).toBeInTheDocument();
  });

  // ─── Global-scope live-lint (Issue #571 Task 2) ────────────────────────────
  //
  // The metric-creation form renders MetricqlSection with experimentId={null}
  // because a metric is not yet bound to an experiment at creation time.
  // The linter must still activate and forward an empty experiment_id to M5's
  // ValidateMetricql RPC, which builds the known-metric set from the global
  // catalog (Task 1 of #571). These tests mirror the new-metric form's call
  // shape and verify the squiggle path is wired end-to-end.

  test('linter activates and surfaces diagnostics when experimentId is null', async () => {
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
      <MetricqlSection
        value="@unknown_metric"
        onChange={() => {}}
        experimentId={null}
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');

    // The CM6 linter has a 500ms `delay` debounce. Wait for the RPC to fire.
    await waitFor(
      () => {
        expect(mocked).toHaveBeenCalled();
      },
      { timeout: 3000 },
    );

    // Verify the empty wire signal that M5 interprets as global scope.
    expect(mocked).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: '@unknown_metric' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );

    // After the response resolves, CM6 paints lint markers into the DOM as
    // `.cm-lintRange` (with severity-specific subclass `.cm-lintRange-error`).
    // This is the squiggle assertion.
    await waitFor(
      () => {
        const editor = screen.getByTestId('metricql-editor');
        const lintRange = editor.querySelector('.cm-lintRange-error');
        expect(lintRange).not.toBeNull();
      },
      { timeout: 3000 },
    );

    // MetricqlSection normalises `experimentId ?? ''` for MetricqlPreview
    // (which requires a non-nullable string). Verify the preview stub
    // receives the empty wire signal, mirroring the linter's RPC wire shape.
    const previewToggle = screen.getByTestId('metricql-preview-toggle');
    expect(previewToggle.getAttribute('data-experiment-id')).toBe('');
  });

  test('linter activates when experimentId prop is omitted entirely', async () => {
    // Omitting the prop (vs. passing null) exercises the `undefined` codepath
    // through the optional `experimentId?: string | null` type.
    const { validateMetricql } = await import('@/lib/api');
    const mocked = vi.mocked(validateMetricql);
    mocked.mockResolvedValueOnce({
      diagnostics: [],
      referencedMetricIds: [],
    });

    render(
      <MetricqlSection
        value="@another"
        onChange={() => {}}
        knownMetricIds={[]}
      />,
    );

    await screen.findByTestId('metricql-editor');

    await waitFor(
      () => {
        expect(mocked).toHaveBeenCalled();
      },
      { timeout: 3000 },
    );

    expect(mocked).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: '@another' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });
});
