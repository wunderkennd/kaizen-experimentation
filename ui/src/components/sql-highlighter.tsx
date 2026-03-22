'use client';

import { useState } from 'react';
import { Highlight, themes } from 'prism-react-renderer';

interface SqlHighlighterProps {
  sql: string;
}

export function SqlHighlighter({ sql }: SqlHighlighterProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(sql);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy text: ', err);
    }
  };

  return (
    <div className="group relative">
      <button
        type="button"
        onClick={handleCopy}
        className="absolute right-2 top-4 z-10 rounded border border-gray-300 bg-white px-2 py-1 text-[10px] font-medium text-gray-600 opacity-0 shadow-sm transition-opacity hover:bg-gray-50 group-hover:opacity-100 focus:opacity-100 focus:outline-none focus:ring-2 focus:ring-indigo-500"
        aria-label="Copy SQL to clipboard"
      >
        {copied ? 'Copied!' : 'Copy'}
      </button>
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
