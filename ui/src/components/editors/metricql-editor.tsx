'use client';

import { useEffect, useState, useRef } from 'react';
import MonacoEditor, { useMonaco } from '@monaco-editor/react';
import { validateMetricql, listMetricDefinitions, type MetricqlDiagnostic } from '@/lib/api';

interface MetricqlEditorProps {
  value: string;
  onChange: (next: string) => void;
  disabled?: boolean;
  metricId?: string;
}

export function MetricqlEditor({ value, onChange, disabled, metricId }: MetricqlEditorProps) {
  const monaco = useMonaco();
  const [metricSuggestions, setMetricSuggestions] = useState<string[]>([]);
  const [isValidating, setIsValidating] = useState(false);
  const [validationResult, setValidationResult] = useState<{
    isValid: boolean;
    diagnostics: MetricqlDiagnostic[];
  } | null>(null);
  
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const editorRef = useRef<any>(null);

  // 1. Fetch existing metrics for autocompletion of @metric_id
  useEffect(() => {
    listMetricDefinitions()
      .then((res) => {
        setMetricSuggestions(res.metrics.map((m) => m.metricId));
      })
      .catch((err) => {
        console.error('Failed to load metric IDs for auto-completion:', err);
      });
  }, []);

  // 2. Register metricql language in Monaco
  useEffect(() => {
    if (!monaco) return;

    // Register a new language if it's not already registered
    const languages = monaco.languages.getLanguages();
    if (!languages.some((lang) => lang.id === 'metricql')) {
      monaco.languages.register({ id: 'metricql' });

      // Tokenizer / Syntax Highlighting configuration
      monaco.languages.setMonarchTokensProvider('metricql', {
        keywords: [
          'mean', 'sum', 'count', 'count_distinct', 'proportion', 'percentile', 'ratio',
          'where', 'and', 'within', 'days', 'hours', 'of', 'exposure'
        ],
        tokenizer: {
          root: [
            // Keywords
            [/[a-zA-Z_]\w*/, {
              cases: {
                '@keywords': 'keyword',
                '@default': 'identifier'
              }
            }],
            // Variables (@metric_id)
            [/@[a-zA-Z_]\w*/, 'variable'],
            // Numbers
            [/\d+/, 'number'],
            // Strings
            [/"([^"\\]|\\.)*"/, 'string'],
            [/'([^'\\]|\\.)*'/, 'string'],
            // Comments
            [/#.*/, 'comment'],
            [/\/\/.*$/, 'comment'],
          ]
        }
      });

      // Language configuration (matching brackets, comments, etc.)
      monaco.languages.setLanguageConfiguration('metricql', {
        comments: {
          lineComment: '//',
        },
        brackets: [
          ['{', '}'],
          ['[', ']'],
          ['(', ')']
        ],
        autoClosingPairs: [
          { open: '{', close: '}' },
          { open: '[', close: ']' },
          { open: '(', close: ')' },
          { open: '"', close: '"', notIn: ['string'] },
          { open: '\'', close: '\'', notIn: ['string', 'comment'] },
        ]
      });
    }
  }, [monaco]);

  // 3. Register autocomplete provider dynamically with loaded metric suggestions
  useEffect(() => {
    if (!monaco) return;

    const provider = monaco.languages.registerCompletionItemProvider('metricql', {
      triggerCharacters: ['@'],
      provideCompletionItems: (model, position) => {
        const word = model.getWordUntilPosition(position);
        const range = {
          startLineNumber: position.lineNumber,
          endLineNumber: position.lineNumber,
          startColumn: word.startColumn,
          endColumn: word.endColumn
        };

        const lineContent = model.getLineContent(position.lineNumber);
        const textBefore = lineContent.substring(0, position.column - 1);

        // If the user typed '@', suggest metric references
        if (textBefore.endsWith('@')) {
          // Adjust range to replace the '@' character
          const adjustedRange = {
            ...range,
            startColumn: range.startColumn - 1
          };
          return {
            suggestions: metricSuggestions.map((id) => ({
              label: `@${id}`,
              kind: monaco.languages.CompletionItemKind.Variable,
              insertText: `@${id}`,
              range: adjustedRange,
              detail: 'Metric Reference',
              documentation: `References the active metric: ${id}`
            }))
          };
        }

        // Default: suggest keywords
        const keywords = [
          'mean', 'sum', 'count', 'count_distinct', 'proportion', 'percentile', 'ratio',
          'where', 'and', 'within', 'days', 'hours', 'of', 'exposure'
        ];

        return {
          suggestions: keywords.map((kw) => ({
            label: kw,
            kind: monaco.languages.CompletionItemKind.Keyword,
            insertText: kw,
            range: range
          }))
        };
      }
    });

    return () => {
      provider.dispose();
    };
  }, [monaco, metricSuggestions]);

  // 4. Handle expression validation
  const runValidation = async (expr: string) => {
    if (!expr || expr.trim() === '') {
      setValidationResult(null);
      if (editorRef.current && monaco) {
        monaco.editor.setModelMarkers(editorRef.current.getModel(), 'metricql', []);
      }
      return;
    }

    setIsValidating(true);
    try {
      const res = await validateMetricql(expr, metricId);
      setValidationResult(res);

      if (editorRef.current && monaco) {
        const model = editorRef.current.getModel();
        const markers = res.diagnostics.map((d) => {
          // Find the word bounds around the line/column to make the squiggle precise
          const lineContent = model.getLineContent(d.line) || '';
          let endCol = d.column + 5; // default length
          
          // Match next word or boundary
          const remainder = lineContent.substring(d.column - 1);
          const wordMatch = remainder.match(/^\w+/);
          if (wordMatch) {
            endCol = d.column + wordMatch[0].length;
          }

          return {
            message: d.message,
            severity: d.severity === 2 ? monaco.MarkerSeverity.Warning : monaco.MarkerSeverity.Error,
            startLineNumber: d.line || 1,
            startColumn: d.column || 1,
            endLineNumber: d.line || 1,
            endColumn: endCol,
          };
        });

        monaco.editor.setModelMarkers(model, 'metricql', markers);
      }
    } catch (err) {
      console.error('Validation API error:', err);
    } finally {
      setIsValidating(false);
    }
  };

  // Debounce editor changes to avoid slamming the validation API
  const handleEditorChange = (val: string | undefined) => {
    const nextVal = val || '';
    onChange(nextVal);

    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }

    timerRef.current = setTimeout(() => {
      runValidation(nextVal);
    }, 500);
  };

  // Clean up timers on unmount
  useEffect(() => {
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    };
  }, []);

  const handleEditorDidMount = (editor: any) => {
    editorRef.current = editor;
    // Initial validation pass
    if (value) {
      runValidation(value);
    }
  };

  return (
    <div className="flex flex-col gap-3 rounded-xl border border-gray-200 bg-white shadow-sm overflow-hidden">
      {/* Editor Header Panel with Rich Glassmorphism Styling */}
      <div className="flex items-center justify-between border-b border-gray-100 bg-gradient-to-r from-slate-900 to-slate-800 px-4 py-3 text-white">
        <div className="flex items-center gap-2">
          <span className="flex h-2.5 w-2.5 rounded-full bg-indigo-400 animate-pulse"></span>
          <h3 className="text-sm font-semibold tracking-wide text-slate-100">MetricQL Compiler Playground</h3>
        </div>

        {/* Validation Status Badges with Subtle Smooth Hover effects */}
        <div className="flex items-center gap-2 text-xs">
          {isValidating && (
            <span className="flex items-center gap-1.5 rounded-full bg-slate-800 border border-slate-700 px-2.5 py-1 font-medium text-indigo-300">
              <svg className="h-3.5 w-3.5 animate-spin" viewBox="0 0 24 24" fill="none">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
              </svg>
              Compiling...
            </span>
          )}

          {!isValidating && validationResult && (
            validationResult.isValid ? (
              <span className="flex items-center gap-1 rounded-full bg-emerald-950/80 border border-emerald-500/30 px-2.5 py-1 font-medium text-emerald-400">
                <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2.5">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                </svg>
                Syntax Valid
              </span>
            ) : (
              <span className="flex items-center gap-1 rounded-full bg-rose-950/80 border border-rose-500/30 px-2.5 py-1 font-medium text-rose-400 shadow-sm shadow-rose-900/20">
                <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2.5">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
                {validationResult.diagnostics.length} {validationResult.diagnostics.length === 1 ? 'Error' : 'Errors'}
              </span>
            )
          )}
        </div>
      </div>

      {/* Monaco Code Editor container */}
      <div className="relative border-b border-gray-100 bg-slate-50" style={{ height: '240px' }}>
        <MonacoEditor
          height="100%"
          language="metricql"
          theme="vs-dark"
          value={value}
          onChange={handleEditorChange}
          onMount={handleEditorDidMount}
          options={{
            minimap: { enabled: false },
            fontSize: 13,
            lineNumbers: 'on',
            scrollbar: {
              vertical: 'visible',
              horizontal: 'auto',
              verticalScrollbarSize: 8,
              horizontalScrollbarSize: 8
            },
            tabSize: 2,
            insertSpaces: true,
            scrollBeyondLastLine: false,
            readOnly: disabled,
            fontFamily: 'Fira Code, JetBrains Mono, source-code-pro, Menlo, Monaco, Consolas, monospace',
            cursorBlinking: 'smooth',
            cursorSmoothCaretAnimation: 'on',
            smoothScrolling: true,
            padding: { top: 12, bottom: 12 }
          }}
        />
      </div>

      {/* Detailed Diagnostic Panel */}
      {validationResult && !validationResult.isValid && validationResult.diagnostics.length > 0 && (
        <div className="bg-rose-50 border-t border-rose-100 px-4 py-3 text-xs text-rose-800 transition-all duration-300">
          <h4 className="font-semibold text-rose-950 mb-1.5 flex items-center gap-1.5">
            <svg className="h-4 w-4 text-rose-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="2">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
            Compilation Failures ({validationResult.diagnostics.length})
          </h4>
          <ul className="flex flex-col gap-1.5 pl-5 list-disc font-mono">
            {validationResult.diagnostics.map((diag, idx) => (
              <li key={idx} className="leading-relaxed">
                <span className="font-bold text-rose-900">[Line {diag.line}, Col {diag.column}]:</span>{' '}
                {diag.message}
              </li>
            ))}
          </ul>
        </div>
      )}
      
      {/* Help Instructions Footer */}
      <div className="px-4 py-3 bg-slate-50 text-[11px] text-slate-500 leading-relaxed font-sans border-t border-slate-100/50">
        <p>
          💡 <strong>MetricQL Help:</strong> Enter metric definitions using keywords like{' '}
          <code className="bg-slate-200/80 text-slate-700 px-1 rounded font-mono">mean</code>,{' '}
          <code className="bg-slate-200/80 text-slate-700 px-1 rounded font-mono">sum</code>, or{' '}
          <code className="bg-slate-200/80 text-slate-700 px-1 rounded font-mono">ratio</code>.
          Reference other metrics using <code className="bg-slate-200/80 text-slate-700 px-1 rounded font-mono">@metric_id</code> for composite flows.
        </p>
      </div>
    </div>
  );
}
