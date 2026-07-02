'use client';

import { useEffect, type RefObject } from 'react';

/**
 * A hook that focuses a given input element when '/', Cmd+K, or Ctrl+K is pressed,
 * unless the user is already typing in an input, textarea, or contenteditable element.
 * Also allows blurring the input with the Escape key.
 */
export function useSearchShortcut(inputRef: RefObject<HTMLInputElement>) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isTyping =
        ['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName || '') ||
        (document.activeElement as HTMLElement)?.isContentEditable;

      // Focus shortcut: '/', Cmd+K, or Ctrl+K
      if (
        (e.key === '/' || ((e.metaKey || e.ctrlKey) && e.key === 'k')) &&
        !isTyping
      ) {
        e.preventDefault();
        inputRef.current?.focus();
      }

      // Blur shortcut: Escape
      if (e.key === 'Escape' && document.activeElement === inputRef.current) {
        inputRef.current?.blur();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [inputRef]);
}
