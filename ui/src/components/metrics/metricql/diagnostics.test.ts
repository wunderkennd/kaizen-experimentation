/**
 * Tests for MetricQL inline diagnostics linter (B4, ADR-026 Phase 2 #436).
 *
 * Strategy: test `metricqlLintSource` directly (the exported inner function)
 * rather than the opaque Extension wrapper. This mirrors the B3 pattern of
 * exporting `metricqlCompletionSource` for direct testing.
 *
 * The source function signature is: `(view: EditorView) => Promise<Diagnostic[]>`
 * We pass a minimal mock object that satisfies `{ state: { doc: { toString() } } }`
 * so tests never need a real EditorView.
 */

import { describe, test, expect, vi, beforeEach, afterEach } from 'vitest';
import { metricqlLintSource } from './diagnostics';
import type { ValidateMetricqlRpcResponse } from './diagnostics';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Minimal EditorView mock — only `.state.doc.toString()` is used. */
function makeView(doc: string) {
  return {
    state: {
      doc: {
        toString: () => doc,
      },
    },
  } as unknown as import('@codemirror/view').EditorView;
}

/** Build a fully-resolved ValidateMetricqlRpcResponse with no diagnostics. */
function okResponse(overrides: Partial<ValidateMetricqlRpcResponse> = {}): ValidateMetricqlRpcResponse {
  return {
    diagnostics: [],
    referencedMetricIds: [],
    ...overrides,
  };
}

/** Build a mock validate function that returns a fixed response. */
function mockValidate(response: ValidateMetricqlRpcResponse | null) {
  return vi.fn().mockResolvedValue(response);
}

// ---------------------------------------------------------------------------
// Suite
// ---------------------------------------------------------------------------

describe('metricqlLintSource', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  // -------------------------------------------------------------------------
  // Empty source fast-path
  // -------------------------------------------------------------------------

  test('returns empty array for empty document without calling validate', async () => {
    const validate = mockValidate(okResponse());
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView(''));
    expect(result).toEqual([]);
    expect(validate).not.toHaveBeenCalled();
  });

  test('returns empty array for whitespace-only document without calling validate', async () => {
    const validate = mockValidate(okResponse());
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('   \n  '));
    expect(result).toEqual([]);
    expect(validate).not.toHaveBeenCalled();
  });

  // -------------------------------------------------------------------------
  // Happy path — diagnostic mapping
  // -------------------------------------------------------------------------

  test('returns empty array when server returns no diagnostics', async () => {
    const validate = mockValidate(okResponse());
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('mean(@watch_time)'));
    expect(result).toEqual([]);
  });

  test('maps a single error diagnostic from server response', async () => {
    const validate = mockValidate(okResponse({
      diagnostics: [{
        severity: 1,
        message: 'unknown metric @foo',
        span: { startOffset: 5, endOffset: 9, line: 1, column: 6 },
      }],
    }));
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('mean(@foo)'));
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({
      from: 5,
      to: 9,
      severity: 'error',
      message: 'unknown metric @foo',
    });
  });

  test('maps severity 2 (warning) correctly', async () => {
    const validate = mockValidate(okResponse({
      diagnostics: [{
        severity: 2,
        message: 'deprecated operator',
        span: { startOffset: 0, endOffset: 4, line: 1, column: 1 },
      }],
    }));
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('mean(@ctr)'));
    expect(result).toHaveLength(1);
    expect(result[0].severity).toBe('warning');
  });

  test('maps severity 0 (unspecified) and 1 (error) both to error', async () => {
    const validate = mockValidate(okResponse({
      diagnostics: [
        { severity: 0, message: 'unspecified', span: { startOffset: 0, endOffset: 1, line: 1, column: 1 } },
        { severity: 1, message: 'error', span: { startOffset: 2, endOffset: 5, line: 1, column: 3 } },
      ],
    }));
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('mean(@ctr)'));
    expect(result[0].severity).toBe('error');
    expect(result[1].severity).toBe('error');
  });

  test('maps multiple diagnostics preserving order', async () => {
    const validate = mockValidate(okResponse({
      diagnostics: [
        { severity: 1, message: 'first error', span: { startOffset: 0, endOffset: 4, line: 1, column: 1 } },
        { severity: 2, message: 'second warning', span: { startOffset: 5, endOffset: 9, line: 1, column: 6 } },
      ],
    }));
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('mean(@ctr)'));
    expect(result).toHaveLength(2);
    expect(result[0].message).toBe('first error');
    expect(result[1].message).toBe('second warning');
  });

  test('handles null span — falls back to offset 0 with to=1', async () => {
    const validate = mockValidate(okResponse({
      diagnostics: [{
        severity: 1,
        message: 'parse error',
        span: null,
      }],
    }));
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('bad expression'));
    expect(result).toHaveLength(1);
    expect(result[0].from).toBe(0);
    // to must be > from so CM6 renders the underline
    expect(result[0].to).toBeGreaterThan(result[0].from);
  });

  test('passes experimentId and expression to validate', async () => {
    const validate = mockValidate(okResponse());
    const source = metricqlLintSource({ experimentId: 'exp-42', validateFn: validate });

    await source(makeView('mean(@ctr)'));
    expect(validate).toHaveBeenCalledWith(
      { experimentId: 'exp-42', metricqlExpression: 'mean(@ctr)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  // -------------------------------------------------------------------------
  // Global-scope normalisation (Issue #571 Task 2)
  //
  // The metric-creation form has no experimentId. M5's ValidateMetricql handler
  // now treats an empty experiment_id string as global scope (Task 1 of #571).
  // The linter accepts string | null | undefined and normalises to '' at the
  // RPC boundary so the wire format stays a plain string matching the proto.
  // -------------------------------------------------------------------------

  test('normalises null experimentId to empty string at the RPC boundary', async () => {
    const validate = mockValidate(okResponse());
    const source = metricqlLintSource({ experimentId: null, validateFn: validate });

    await source(makeView('mean(@ctr)'));
    expect(validate).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: 'mean(@ctr)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  test('normalises undefined experimentId to empty string at the RPC boundary', async () => {
    const validate = mockValidate(okResponse());
    const source = metricqlLintSource({ experimentId: undefined, validateFn: validate });

    await source(makeView('mean(@ctr)'));
    expect(validate).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: 'mean(@ctr)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  test('passes empty-string experimentId through unchanged', async () => {
    const validate = mockValidate(okResponse());
    const source = metricqlLintSource({ experimentId: '', validateFn: validate });

    await source(makeView('mean(@ctr)'));
    expect(validate).toHaveBeenCalledWith(
      { experimentId: '', metricqlExpression: 'mean(@ctr)' },
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  // -------------------------------------------------------------------------
  // AbortController — cancel-in-flight
  // -------------------------------------------------------------------------

  test('aborts the previous in-flight request when invoked a second time', async () => {
    let firstSignal: AbortSignal | null = null;
    let callCount = 0;

    // First call: hangs until its signal is aborted (then resolves null).
    // Second call: resolves immediately with an empty response.
    const validate = vi.fn().mockImplementation((_args, opts) => {
      callCount++;
      if (callCount === 1) {
        firstSignal = opts.signal;
        return new Promise<ValidateMetricqlRpcResponse | null>((resolve) => {
          opts.signal.addEventListener('abort', () => resolve(null));
        });
      }
      // Second call — resolves right away
      return Promise.resolve(okResponse());
    });

    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    // Fire first call — don't await (it hangs waiting for abort)
    const first = source(makeView('mean(@ctr)'));

    // Fire second call — this aborts the first and resolves immediately
    const second = source(makeView('mean(@watch_time)'));

    const [firstResult, secondResult] = await Promise.all([first, second]);

    // First call was aborted — returns []
    expect(firstResult).toEqual([]);
    expect(firstSignal!.aborted).toBe(true);

    // Second call resolved normally
    expect(secondResult).toEqual([]);
  });

  test('returns empty array when validate returns null (abort observed by callee)', async () => {
    const validate = vi.fn().mockResolvedValue(null);
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('mean(@ctr)'));
    expect(result).toEqual([]);
  });

  // -------------------------------------------------------------------------
  // Timeout
  // -------------------------------------------------------------------------

  test('aborts request and logs warning when timeout fires', async () => {
    const consoleWarn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    let capturedSignal: AbortSignal | null = null;
    const hangingValidate = vi.fn().mockImplementation((_args, opts) => {
      capturedSignal = opts.signal;
      // When the signal is aborted (by our timeout), resolve to null.
      return new Promise<null>((resolve) => {
        opts.signal.addEventListener('abort', () => resolve(null));
      });
    });

    const source = metricqlLintSource({
      experimentId: 'exp-1',
      validateFn: hangingValidate,
      timeoutMs: 2000,
    });

    const resultPromise = source(makeView('mean(@ctr)'));

    // Advance fake timers past the timeout
    await vi.advanceTimersByTimeAsync(2001);

    const result = await resultPromise;

    expect(result).toEqual([]);
    // Signal should have been aborted by our timer
    expect(capturedSignal!.aborted).toBe(true);
    // Warning should have been logged for TimeoutError
    expect(consoleWarn).toHaveBeenCalledWith(
      expect.stringContaining('live-lint timeout'),
      2000,
    );
  });

  test('does not log timeout warning when aborted by cancel-in-flight (not timeout)', async () => {
    const consoleWarn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    // validate returns null immediately (simulates cancel-in-flight handling in callee)
    const validate = vi.fn().mockResolvedValue(null);
    const source = metricqlLintSource({
      experimentId: 'exp-1',
      validateFn: validate,
      timeoutMs: 2000,
    });

    await source(makeView('mean(@ctr)'));

    // Timeout hasn't fired (0ms elapsed) — no warn expected
    expect(consoleWarn).not.toHaveBeenCalledWith(
      expect.stringContaining('live-lint timeout'),
      expect.any(Number),
    );
  });

  // -------------------------------------------------------------------------
  // Network error
  // -------------------------------------------------------------------------

  test('returns empty array and logs warn on network error (no toast)', async () => {
    const consoleWarn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const validate = vi.fn().mockRejectedValue(new Error('network unreachable'));
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    const result = await source(makeView('mean(@ctr)'));

    expect(result).toEqual([]);
    expect(consoleWarn).toHaveBeenCalledWith(
      expect.stringContaining('live-lint failed'),
      expect.any(Error),
    );
  });

  test('does not throw on network error — always returns Diagnostic[]', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    const validate = vi.fn().mockRejectedValue(new TypeError('fetch failed'));
    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    await expect(source(makeView('mean(@ctr)'))).resolves.toEqual([]);
  });

  // -------------------------------------------------------------------------
  // Default timeout value
  // -------------------------------------------------------------------------

  test('uses 2000ms default timeout when timeoutMs is not specified', async () => {
    const consoleWarn = vi.spyOn(console, 'warn').mockImplementation(() => {});

    let capturedSignal: AbortSignal | null = null;
    const hangingValidate = vi.fn().mockImplementation((_args, opts) => {
      capturedSignal = opts.signal;
      return new Promise<null>((resolve) => {
        opts.signal.addEventListener('abort', () => resolve(null));
      });
    });

    const source = metricqlLintSource({
      experimentId: 'exp-1',
      validateFn: hangingValidate,
      // timeoutMs not set — should default to 2000
    });

    const resultPromise = source(makeView('mean(@ctr)'));

    // Advance just under 2000ms — request should still be in flight
    await vi.advanceTimersByTimeAsync(1999);
    expect(capturedSignal!.aborted).toBe(false);

    // Advance past 2000ms — should fire
    await vi.advanceTimersByTimeAsync(2);
    await resultPromise;

    expect(capturedSignal!.aborted).toBe(true);
  });

  // -------------------------------------------------------------------------
  // Stale-result guard
  // -------------------------------------------------------------------------

  test('stale-result guard — drops result when controller is superseded', async () => {
    // Simulate: first validate resolves, but a second call replaced currentController
    // before the first resolved.
    // This is tricky to test without co-operative timing. We test it by creating
    // two source invocations from the SAME source closure and verifying the second
    // call's response wins.
    let resolveFirst!: (r: ValidateMetricqlRpcResponse | null) => void;
    const firstPromise = new Promise<ValidateMetricqlRpcResponse | null>((res) => {
      resolveFirst = res;
    });

    const callCount = { n: 0 };
    const validate = vi.fn().mockImplementation(() => {
      callCount.n++;
      if (callCount.n === 1) return firstPromise;
      return Promise.resolve(okResponse({ referencedMetricIds: ['winner'] }));
    });

    const source = metricqlLintSource({ experimentId: 'exp-1', validateFn: validate });

    // Start first call — hangs
    const first = source(makeView('first expression'));
    // Start second call — completes
    const second = source(makeView('second expression'));

    // Resolve second first
    const secondResult = await second;
    expect(secondResult).toEqual([]);

    // Now resolve first — it should detect stale controller and return []
    resolveFirst(okResponse({ diagnostics: [{ severity: 1, message: 'stale', span: null }] }));
    const firstResult = await first;
    // First call's result is dropped because currentController was replaced
    expect(firstResult).toEqual([]);
  });
});
