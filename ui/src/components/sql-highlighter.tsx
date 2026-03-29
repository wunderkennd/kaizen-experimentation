'use client';

import { Highlight, themes } from 'prism-react-renderer';
import { CopyButton } from './copy-button';

interface SqlHighlighterProps {
  sql: string;
}

export function SqlHighlighter({ sql }: SqlHighlighterProps) {
  return (
    <div className="group relative">
      <CopyButton
        value={sql}
        label="Copy SQL to clipboard"
        successMessage="SQL copied to clipboard"
        className="absolute right-2 top-4 z-10 opacity-0 transition-opacity group-hover:opacity-100 focus:opacity-100"
      />
      <Highlight theme={themes.github} code={sql} language="sql">
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
