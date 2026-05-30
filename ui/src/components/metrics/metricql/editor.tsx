'use client';

/**
 * MetricQL CodeMirror 6 editor component (B2, ADR-026 Phase 2 #436).
 *
 * Lifecycle mirrors sql-editor.tsx:
 *   - Mount-once EditorView in a useEffect so selection state is preserved.
 *   - A Compartment toggles the `editable` facet at runtime (no teardown).
 *   - A stable onChangeRef avoids recreating the EditorView on every render.
 *   - External value and disabled changes are synced in separate effects.
 *
 * Multi-line editing IS permitted (unlike sql-editor.tsx's single-line mode).
 * Input is capped at maxLength bytes (default 4096 — matches M5 server cap).
 *
 * Props experimentId and knownMetricIds are accepted but unused in B2.
 * B3 will wire them into the autocomplete provider; B4 into the linter.
 * They are declared here so B6 (form integration) can pass them through
 * without an API change.
 */

import { useEffect, useRef } from 'react';
import { Compartment, EditorState } from '@codemirror/state';
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
} from '@codemirror/view';
import { defaultKeymap, indentWithTab } from '@codemirror/commands';
import {
  bracketMatching,
  defaultHighlightStyle,
  syntaxHighlighting,
} from '@codemirror/language';
import { metricql } from './language';
import { metricqlAutocomplete } from './autocomplete';
import { metricqlLinter } from './diagnostics';

export interface MetricqlEditorProps {
  value: string;
  onChange: (next: string) => void;
  ariaLabel: string;
  /** Maximum input length in characters. Defaults to 4096 to match M5 server-side cap. */
  maxLength?: number;
  disabled?: boolean;
  /**
   * Experiment ID for the autocomplete provider (B3) and lint provider (B4).
   * Received but unused in B2; included so B6 can pass it through without API churn.
   */
  experimentId?: string;
  /**
   * Known metric IDs for autocomplete (B3).
   * Received but unused in B2; included so B6 can pass it through without API churn.
   */
  knownMetricIds?: string[];
}

export function MetricqlEditor({
  value,
  onChange,
  ariaLabel,
  maxLength = 4096,
  disabled,
  experimentId,
  knownMetricIds,
}: MetricqlEditorProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  // Compartment allows toggling the `editable` facet without destroying the view.
  const editableCompartmentRef = useRef<Compartment>(new Compartment());
  // Stable ref to latest onChange so the mount-once effect never goes stale.
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);
  // Stable ref for knownMetricIds — the autocomplete source reads this on every
  // trigger, so optimistic cache updates (just-created metrics) are visible
  // without restarting the editor.
  const knownMetricIdsRef = useRef<string[]>(knownMetricIds ?? []);
  useEffect(() => {
    knownMetricIdsRef.current = knownMetricIds ?? [];
  }, [knownMetricIds]);
  // experimentId is captured at mount for the linter. If B6 changes the
  // experimentId, it will remount the editor (key prop change), so this ref
  // is always fresh within a mount lifetime.
  const experimentIdRef = useRef(experimentId);
  useEffect(() => {
    experimentIdRef.current = experimentId;
  }, [experimentId]);

  // Mount once — create the EditorView and attach it to the container div.
  useEffect(() => {
    if (!containerRef.current) return;
    const editableCompartment = editableCompartmentRef.current;
    const state = EditorState.create({
      doc: value,
      extensions: [
        metricql(),
        lineNumbers(),
        highlightActiveLine(),
        bracketMatching(),
        syntaxHighlighting(defaultHighlightStyle),
        keymap.of([...defaultKeymap, indentWithTab]),
        EditorView.lineWrapping,
        // B3: @metric_ref autocomplete. Always included; when knownMetricIds
        // is not provided the ref starts empty and the menu simply never opens.
        metricqlAutocomplete({ getKnownMetricIds: () => knownMetricIdsRef.current }),
        // B4: inline MetricQL diagnostics. Only active when an experimentId is
        // provided — the ValidateMetricql RPC requires one to resolve @metric_ref
        // references in context. When absent, no linter is registered and the
        // editor renders without error underlines (safe for standalone usage).
        ...(experimentIdRef.current
          ? [metricqlLinter({
              experimentId: experimentIdRef.current,
              debounceMs: 500,
              timeoutMs: 2000,
            })]
          : []),
        EditorState.transactionFilter.of((tr) => {
          // Multi-line is ALLOWED (unlike sql-editor.tsx).
          // Reject only transactions that would exceed maxLength.
          if (tr.docChanged) {
            const next = tr.newDoc.toString();
            if (next.length > maxLength) return [];
          }
          return [tr];
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            onChangeRef.current(update.state.doc.toString());
          }
        }),
        editableCompartment.of(EditorView.editable.of(!disabled)),
      ],
    });
    viewRef.current = new EditorView({ state, parent: containerRef.current });
    return () => {
      viewRef.current?.destroy();
      viewRef.current = null;
    };
    // Mount-once: external value/disabled/knownMetricIds changes are handled in
    // the sync effects below.  Re-creating the editor on every change would lose
    // selection state and is expensive.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync external value changes (e.g., form reset from outside).
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (current !== value) {
      view.dispatch({ changes: { from: 0, to: current.length, insert: value } });
    }
  }, [value]);

  // Sync the disabled flag via the Compartment (no EditorView teardown).
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: editableCompartmentRef.current.reconfigure(
        EditorView.editable.of(!disabled),
      ),
    });
  }, [disabled]);

  return (
    <div
      ref={containerRef}
      role="textbox"
      aria-multiline="true"
      aria-label={ariaLabel}
      data-testid="metricql-editor"
      className="min-h-[120px] rounded-md border border-gray-300 bg-white font-mono text-sm shadow-sm focus-within:border-indigo-500 focus-within:ring-1 focus-within:ring-indigo-500"
    />
  );
}
