'use client';

import { useEffect, useRef } from 'react';
import { Compartment, EditorState } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { defaultKeymap } from '@codemirror/commands';
import { sql } from '@codemirror/lang-sql';

export interface SqlEditorProps {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  ariaLabel: string;
  /** Maximum input length. Defaults to 4096 to match the M5 server-side cap. */
  maxLength?: number;
  disabled?: boolean;
}

/**
 * Single-line CodeMirror 6 wrapper for FILTERED_MEAN / WINDOWED_COUNT filter_sql
 * fields. Multi-line input is rejected (newlines are filtered out before onChange
 * is called). For multi-line MetricQL editing (Phase 2 / #436), this component
 * will be extended; the wrapper boundary stays the same.
 *
 * Why CodeMirror over a plain textarea: SQL syntax highlighting helps operators
 * spot typos in filter predicates before the M5 round-trip rejects them.
 */
export function SqlEditor({
  value,
  onChange,
  placeholder,
  ariaLabel,
  maxLength = 4096,
  disabled,
}: SqlEditorProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  // Compartment lets us toggle the `editable` facet at runtime without
  // tearing down the EditorView.
  const editableCompartmentRef = useRef<Compartment>(new Compartment());
  // Hold the latest onChange in a ref so the mount-once effect always sees the
  // current closure without re-creating the EditorView on every render.
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  // Mount once.
  useEffect(() => {
    if (!containerRef.current) return;
    const editableCompartment = editableCompartmentRef.current;
    const state = EditorState.create({
      doc: value,
      extensions: [
        sql(),
        keymap.of(defaultKeymap),
        EditorView.lineWrapping,
        EditorState.transactionFilter.of((tr) => {
          // Single-line: reject any transaction that would introduce newlines
          // or exceed the maxLength cap.
          if (tr.docChanged) {
            const inserted = tr.newDoc.toString();
            if (inserted.includes('\n')) return [];
            if (inserted.length > maxLength) return [];
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
    // Mount-once: external value/disabled changes are handled in the sync
    // effects below. Re-creating the editor on every keystroke would lose
    // selection state and is expensive.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync external value changes (e.g., dispatch resets the form).
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (current !== value) {
      view.dispatch({ changes: { from: 0, to: current.length, insert: value } });
    }
  }, [value]);

  // Sync the disabled flag via the compartment.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: editableCompartmentRef.current.reconfigure(EditorView.editable.of(!disabled)),
    });
  }, [disabled]);

  return (
    <div
      ref={containerRef}
      role="textbox"
      aria-label={ariaLabel}
      data-testid="sql-editor"
      className="min-h-[40px] rounded-md border border-gray-300 bg-white font-mono text-sm shadow-sm focus-within:border-indigo-500 focus-within:ring-1 focus-within:ring-indigo-500"
      data-placeholder={placeholder}
    />
  );
}
