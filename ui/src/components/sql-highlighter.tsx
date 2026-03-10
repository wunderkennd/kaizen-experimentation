'use client';

import { Highlight, themes } from 'prism-react-renderer';

interface SqlHighlighterProps {
  sql: string;
}

export function SqlHighlighter({ sql }: SqlHighlighterProps) {
  return (
    <Highlight theme={themes.github} code={sql} language="sql">
      {({ style, tokens, getLineProps, getTokenProps }) => (
        <pre
          className="mt-2 overflow-x-auto rounded bg-gray-50 p-3 font-mono text-xs"
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
  );
}
