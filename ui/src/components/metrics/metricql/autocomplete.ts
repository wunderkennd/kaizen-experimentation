/**
 * MetricQL @metric_ref autocomplete provider (B3, ADR-026 Phase 2 #436).
 *
 * Provides CodeMirror 6 completion for `@<metric_id>` references in MetricQL
 * expressions. Triggers on `@` typed by the user and filters the cached metric
 * catalog by prefix. No RPC is issued from the completion source — the caller
 * supplies a stable getter (typically a useRef-backed closure reading from
 * the form-shell's already-fetched list, per L6).
 */

import {
  CompletionContext,
  CompletionResult,
  autocompletion,
} from '@codemirror/autocomplete';
import { Extension } from '@codemirror/state';

export interface MetricqlAutocompleteOptions {
  /**
   * Returns the current list of known metric IDs. Called on every completion
   * trigger — must be cheap (read from a ref/store, never fetch). The
   * form-shell keeps this list cached from its ListMetricDefinitions call
   * (per L6); optimistic cache updates (just-created metrics) are visible
   * immediately because the getter is called at trigger time.
   */
  getKnownMetricIds: () => string[];
}

/**
 * Completion source function, exported separately so tests can invoke it
 * directly without having to unwrap the Extension wrapper produced by
 * metricqlAutocomplete().
 */
export function metricqlCompletionSource(opts: MetricqlAutocompleteOptions) {
  return (context: CompletionContext): CompletionResult | null => {
    // Match `@` followed by zero or more valid identifier characters.
    const word = context.matchBefore(/@[a-z0-9_]*/i);
    if (!word) return null;

    // When the cursor is exactly at `@` (nothing typed yet), only activate on
    // an explicit trigger (Ctrl-Space). Avoids spurious popups for `@`
    // characters inside string literals.
    if (word.from === word.to - 1 && !context.explicit) {
      // word.text === '@' with cursor right after it: empty prefix
      if (word.text === '@') return null;
    }
    // Simpler check: from === to means cursor is on `@` itself (before anything)
    if (word.from === word.to && !context.explicit) return null;

    const knownIds = opts.getKnownMetricIds();
    // Strip the leading `@` to get the typed prefix; lowercase for comparison.
    const prefix = word.text.slice(1).toLowerCase();

    const matches = knownIds
      .filter((id) => id.toLowerCase().startsWith(prefix))
      .sort()
      .map((id) => ({
        label: `@${id}`,
        type: 'variable',
        apply: `@${id}`,
      }));

    if (matches.length === 0) return null;

    return {
      from: word.from,
      options: matches,
      // Keep the completion menu open while the user continues typing the
      // @-prefixed token.
      validFor: /^@[a-z0-9_]*$/i,
    };
  };
}

/**
 * CodeMirror 6 Extension that adds MetricQL @metric_ref autocomplete.
 *
 * Usage (inside a MetricqlEditor mount-once useEffect):
 *
 *   extensions.push(metricqlAutocomplete({ getKnownMetricIds: () => knownIdsRef.current }));
 *
 * The `override` option replaces CM6's default word-based completion with the
 * MetricQL-specific source. Additional sources (e.g., keyword completion) can
 * be added alongside by composing a second autocompletion() extension or by
 * passing multiple sources to override.
 */
export function metricqlAutocomplete(opts: MetricqlAutocompleteOptions): Extension {
  return autocompletion({
    override: [metricqlCompletionSource(opts)],
  });
}
