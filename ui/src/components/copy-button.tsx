'use client';

import { useCallback } from 'react';
import { useToast } from '@/lib/toast-context';

interface CopyButtonProps {
  text: string;
  className?: string;
  label?: string;
}

export function CopyButton({ text, className = '', label = 'Copy to clipboard' }: CopyButtonProps) {
  const { addToast } = useToast();

  const handleCopy = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    navigator.clipboard.writeText(text).then(() => {
      addToast('Copied to clipboard', 'success');
    }).catch((err) => {
      addToast(err instanceof Error ? err.message : 'Failed to copy', 'error');
    });
  }, [text, addToast]);

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={`group relative flex h-6 w-6 items-center justify-center rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 focus:outline-none focus:ring-2 focus:ring-indigo-500 ${className}`}
      aria-label={label}
      title={label}
    >
      <svg
        className="h-4 w-4"
        fill="none"
        viewBox="0 0 24 24"
        stroke="currentColor"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M8 5H6a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2v-1M8 5a2 2 0 002 2h2a2 2 0 002-2M8 5a2 2 0 012-2h2a2 2 0 012 2m0 0h2a2 2 0 012 2v3m2 4H10m0 0l3-3m-3 3l3 3"
        />
      </svg>
    </button>
  );
}
