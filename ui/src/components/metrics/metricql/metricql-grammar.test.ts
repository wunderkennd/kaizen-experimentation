/**
 * MetricQL Lezer grammar — corpus parity test (B1, ADR-026 Phase 2 #436).
 *
 * Validates that the generated Lezer parser agrees with the Go/Rust parsers on
 * SYNTACTIC validity for every fixture in test-vectors/metricql_corpus.json.
 *
 * Scope: syntactic only.
 * - valid:true fixtures MUST parse with zero Error nodes.
 * - valid:false fixtures fall into two buckets:
 *   (a) "lex_" / "parse_structural_" — actual tokenizer / grammar failures that
 *       produce Error nodes in the Lezer tree.  The test asserts hasError == true.
 *   (b) Semantic fixtures (unresolved @ref, wrong arg-count) and parser fixtures
 *       that reflect Go/Rust range checks (percentile > 100, window N ≤ 0,
 *       non-integer window) — the Lezer grammar accepts these syntactically.
 *       No assertion is made on hasError for these; the runtime linter (A9 /
 *       ValidateMetricql RPC) catches them server-side.
 *
 * The distinction between (a) and (b) is encoded in SEMANTIC_FIXTURES and
 * PARSE_SEMANTIC_FIXTURES below.
 */

import { parser } from './metricql';
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore — JSON import; resolveJsonModule is enabled in tsconfig.json
import corpusJson from '../../../../../test-vectors/metricql_corpus.json';

// ─── Corpus loading ───────────────────────────────────────────────────────────

interface Fixture {
  name: string;
  source: string;
  valid: boolean;
  expected_refs?: string[];
  expected_error_count?: number;
}

// corpusJson is imported at the top of the file via a JSON static import.
// Vitest + esbuild + resolveJsonModule handles this correctly.
const corpus: Fixture[] = corpusJson as Fixture[];

// ─── Helpers ──────────────────────────────────────────────────────────────────

function countErrorNodes(source: string): number {
  const tree = parser.parse(source);
  let count = 0;
  tree.iterate({
    enter(node) {
      if (node.type.isError) count++;
    },
  });
  return count;
}

/**
 * Fixtures whose valid:false status is due to SEMANTIC constraints that the
 * Lezer grammar intentionally does NOT enforce (the grammar only enforces
 * syntactic structure).  These are accepted cleanly by the Lezer parser.
 *
 * Semantic errors are caught server-side via the ValidateMetricql RPC (A9).
 */
const SEMANTIC_FIXTURES = new Set([
  // Bare metric refs or literals in Expression position are syntactically
  // valid composite expressions; the Go/Rust analyzer adds the constraint
  // that a top-level expression must be an aggregation or a multi-operand
  // composite (not a naked @ref or literal).
  'semantic_bare_ref',
  'semantic_bare_literal',

  // count(login.foo) and mean(heartbeat) are syntactically valid aggregations;
  // the Go/Rust analyzer enforces that count/proportion/count_distinct must
  // not have a field suffix, and mean/sum must have one.
  'semantic_count_with_field',
  'semantic_mean_no_field',

  // The following are named "parse_*" in the corpus but the invalid condition
  // is a RANGE CHECK applied by the Go/Rust parser after syntactic parsing —
  // not a structural grammar error.  The Lezer grammar accepts these because
  // it has no mechanism for numeric range validation.
  'parse_percentile_out_of_range', // Go rejects pct > 100; grammar accepts any Number
  'parse_window_zero',             // Go rejects N == 0; grammar accepts any Number
  'parse_window_non_integer',      // Go rejects 1.5; grammar accepts any Number
]);

// ─── Tests ────────────────────────────────────────────────────────────────────

describe('MetricQL Lezer grammar — corpus parity', () => {
  describe('valid fixtures parse with zero error nodes', () => {
    const validCases = corpus.filter((f) => f.valid);
    test.each(validCases)('$name', ({ source }) => {
      expect(countErrorNodes(source)).toBe(0);
    });
  });

  describe('lex/parse error fixtures produce error nodes', () => {
    const lexParseCases = corpus.filter(
      (f) => !f.valid && !SEMANTIC_FIXTURES.has(f.name)
    );
    test.each(lexParseCases)('$name', ({ source }) => {
      expect(countErrorNodes(source)).toBeGreaterThan(0);
    });
  });

  describe('semantic fixtures are syntactically accepted (no error nodes)', () => {
    const semanticCases = corpus.filter(
      (f) => !f.valid && SEMANTIC_FIXTURES.has(f.name)
    );
    test.each(semanticCases)('$name', ({ source }) => {
      // The grammar accepts these; errors are caught by the runtime linter.
      expect(countErrorNodes(source)).toBe(0);
    });
  });

  test('corpus fixture count matches expected total', () => {
    // Sanity check: the corpus should have exactly 39 fixtures.
    // Update this constant if the corpus is intentionally extended.
    expect(corpus).toHaveLength(39);
  });
});
