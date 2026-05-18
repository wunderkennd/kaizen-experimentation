'use client';

import { Highlight, themes } from 'prism-react-renderer';
import { CopyButton } from './copy-button';

interface SqlHighlighterProps {
  sql: string;
  /**
   * Prism language token. Defaults to `'sql'` for backward compatibility with
   * the existing read-only SQL displays. ADR-026 Phase 1 reuses this for the
   * proto JSON preview in the metric creation form (`language="json"`).
   */
  language?: 'sql' | 'json';
  copyLabel?: string;
  copySuccessMessage?: string;
}

export function SqlHighlighter({
  sql,
  language = 'sql',
  copyLabel,
  copySuccessMessage,
}: SqlHighlighterProps) {
  const isSql = language === 'sql';
  return (
    <div className="group relative">
      <CopyButton
        value={sql}
        label={copyLabel ?? (isSql ? 'Copy SQL to clipboard' : 'Copy to clipboard')}
        successMessage={copySuccessMessage ?? (isSql ? 'SQL copied to clipboard' : 'Copied to clipboard')}
        className="absolute right-2 top-4 z-10 opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100"
      />
      <Highlight theme={themes.github} code={sql} language={language}>
        {({ style, tokens, getLineProps, getTokenProps }) => (
          <pre
            className="mt-2 overflow-x-auto rounded bg-gray-50 p-3 pr-14 font-mono text-xs"
            style={{ ...style, backgroundColor: 'transparent' }}
          >
            {tokens.map((line, i) => (
              <div key={i} {...getLineProps({ line })}>
                {line.map((token, key) => (
                  <span key={key} {...getTokenProps({ token })} />
                ))}
              </div>
            ))}
          </pre>
        )}
      </Highlight>
    </div>
  );
}
