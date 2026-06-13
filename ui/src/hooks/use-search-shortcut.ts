'use client';

import { useEffect, type RefObject } from 'react';

/**
 * A hook that focuses a given input element when the '/', Cmd+K, or Ctrl+K key is pressed,
 * unless the user is already typing in an input, textarea, or contenteditable element.
 * Also blurs the input when the Escape key is pressed.
 */
export function useSearchShortcut(inputRef: RefObject<HTMLInputElement>) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isTyping =
        ['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName || '') ||
        (document.activeElement as HTMLElement)?.isContentEditable;

      // Focus search: '/' (if not typing) or Cmd+K / Ctrl+K (anywhere)
      if (
        (e.key === '/' && !isTyping) ||
        ((e.metaKey || e.ctrlKey) && e.key === 'k')
      ) {
        e.preventDefault();
        inputRef.current?.focus();
      }

      // Blur search: 'Escape' if input is focused
      if (e.key === 'Escape' && document.activeElement === inputRef.current) {
        inputRef.current?.blur();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [inputRef]);
}
