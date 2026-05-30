/**
 * Tests for MetricQL @metric_ref autocomplete provider (B3, ADR-026 Phase 2 #436).
 */

import { describe, test, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { CompletionContext } from '@codemirror/autocomplete';
import { metricqlCompletionSource } from './autocomplete';
import { metricql } from './language';

/** Build a CompletionContext at the given cursor position. */
function makeContext(doc: string, pos: number, explicit = false): CompletionContext {
  const state = EditorState.create({ doc, extensions: [metricql()] });
  return new CompletionContext(state, pos, explicit);
}

const KNOWN = ['watch_time', 'ctr', 'engagement', 'click_count', 'signup_rate'];

describe('metricqlCompletionSource', () => {
  test('triggers on @ with empty prefix when explicit (Ctrl-Space)', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    // cursor is positioned right after `@` — one character in
    const ctx = makeContext('@', 1, /* explicit */ true);
    const result = src(ctx);
    expect(result).not.toBeNull();
    const labels = result!.options.map((o) => o.label);
    expect(labels).toEqual(
      expect.arrayContaining([
        '@click_count',
        '@ctr',
        '@engagement',
        '@signup_rate',
        '@watch_time',
      ]),
    );
    // All five metrics returned
    expect(labels).toHaveLength(5);
  });

  test('does NOT trigger on bare @ without explicit flag', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    const ctx = makeContext('@', 1, /* explicit */ false);
    const result = src(ctx);
    expect(result).toBeNull();
  });

  test('triggers without explicit once any character follows @', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    // `@w` — prefix is 'w'
    const ctx = makeContext('@w', 2, false);
    const result = src(ctx);
    expect(result).not.toBeNull();
    expect(result!.options.map((o) => o.label)).toEqual(['@watch_time']);
  });

  test('filters by @-prefix (prefix match, not substring)', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    // `@wat` — should match watch_time only, not engagement
    const ctx = makeContext('@wat', 4);
    const result = src(ctx);
    expect(result).not.toBeNull();
    expect(result!.options.map((o) => o.label)).toEqual(['@watch_time']);
  });

  test('filters case-insensitively', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    const ctx = makeContext('@WAT', 4);
    const result = src(ctx);
    expect(result).not.toBeNull();
    expect(result!.options.map((o) => o.label)).toEqual(['@watch_time']);
  });

  test('results are sorted alphabetically', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    // `@c` matches ctr, click_count — alphabetical order: click_count, ctr
    const ctx = makeContext('@c', 2, true);
    const result = src(ctx);
    expect(result).not.toBeNull();
    expect(result!.options.map((o) => o.label)).toEqual(['@click_count', '@ctr']);
  });

  test('apply value includes the @ sigil', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    const ctx = makeContext('@ctr', 4);
    const result = src(ctx);
    expect(result).not.toBeNull();
    const opt = result!.options[0];
    expect(opt.apply).toBe('@ctr');
    expect(opt.type).toBe('variable');
  });

  test('from is set to the position of @', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    // `mean(@wat` — @ is at position 5
    const ctx = makeContext('mean(@wat', 9);
    const result = src(ctx);
    expect(result).not.toBeNull();
    expect(result!.from).toBe(5);
    expect(result!.options.map((o) => o.label)).toEqual(['@watch_time']);
  });

  test('returns null when no @ context in scope', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    const ctx = makeContext('mean(', 5);
    const result = src(ctx);
    expect(result).toBeNull();
  });

  test('returns null when prefix matches nothing in the catalog', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    const ctx = makeContext('@xyz', 4);
    const result = src(ctx);
    expect(result).toBeNull();
  });

  test('returns null when catalog is empty', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => [] });
    const ctx = makeContext('@watch', 6);
    const result = src(ctx);
    expect(result).toBeNull();
  });

  test('validFor regex keeps menu open during continued typing', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => KNOWN });
    const ctx = makeContext('@watch_t', 8);
    const result = src(ctx);
    expect(result).not.toBeNull();
    // The validFor regex should match the partially typed @-token
    const validFor = result!.validFor as RegExp;
    expect(validFor.test('@watch_t')).toBe(true);
    expect(validFor.test('@watch_time')).toBe(true);
    // Should NOT keep the menu open when the user typed something that moved
    // past the @-token (e.g., typed a space or operator)
    expect(validFor.test('@watch_time ')).toBe(false);
  });

  test('reflects optimistic cache update — just-created metric appears immediately', () => {
    let cache = [...KNOWN];
    const src = metricqlCompletionSource({ getKnownMetricIds: () => cache });

    // Initially 'new_metric' is absent
    let ctx = makeContext('@new', 4);
    let result = src(ctx);
    expect(result).toBeNull();

    // Operator just created 'new_metric'; cache is updated optimistically
    cache = [...cache, 'new_metric'];

    // Autocomplete picks it up on the next trigger — getter is called at
    // trigger time, not at extension-mount time
    ctx = makeContext('@new', 4);
    result = src(ctx);
    expect(result).not.toBeNull();
    expect(result!.options[0].label).toBe('@new_metric');
  });

  test('handles metrics with numeric characters in id', () => {
    const src = metricqlCompletionSource({ getKnownMetricIds: () => ['p90_latency', 'p99_latency'] });
    const ctx = makeContext('@p9', 3);
    const result = src(ctx);
    expect(result).not.toBeNull();
    expect(result!.options.map((o) => o.label)).toEqual(['@p90_latency', '@p99_latency']);
  });
});
