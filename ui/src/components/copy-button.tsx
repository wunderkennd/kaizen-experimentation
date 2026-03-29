'use client';

import { useState, useCallback, useEffect } from 'react';
import { useToast } from '@/lib/toast-context';

interface CopyButtonProps {
  value: string;
  label?: string;
  className?: string;
  successMessage?: string;
}

/**
 * A reusable button for copying text to the clipboard.
 * Provides visual feedback via a temporary icon change and a toast notification.
 */
export function CopyButton({
  value,
  label = 'Copy to clipboard',
  className = '',
  successMessage = 'Copied to clipboard',
}: CopyButtonProps) {
  const [copied, setCopied] = useState(false);
  const { addToast } = useToast();

  const handleCopy = useCallback(async (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation(); // Prevent triggering parent click handlers (e.g., in table rows)

    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      addToast(successMessage, 'success');
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy text: ', err);
      addToast('Failed to copy to clipboard', 'error');
    }
  }, [value, addToast, successMessage]);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={`group relative flex h-6 w-6 items-center justify-center rounded-md text-gray-400 hover:bg-gray-100 hover:text-gray-600 focus:outline-none focus:ring-2 focus:ring-indigo-500 disabled:opacity-50 ${className}`}
      aria-label={label}
      title={label}
    >
      {copied ? (
        <svg
          className="h-4 w-4 text-green-600"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M5 13l4 4L19 7"
          />
        </svg>
      ) : (
        <svg
          className="h-4 w-4"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          aria-hidden="true"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M8 5H6a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2v-1M8 5a2 2 0 002 2h2a2 2 0 002-2M8 5a2 2 0 012-2h2a2 2 0 012 2m0 0h2a2 2 0 012 2v3m2 4H10m0 0l3-3m-3 3l3 3"
          />
        </svg>
      )}
      {copied && (
        <span className="absolute -top-8 left-1/2 -translate-x-1/2 whitespace-nowrap rounded bg-gray-800 px-2 py-1 text-[10px] font-medium text-white">
          Copied!
        </span>
      )}
    </button>
  );
}
