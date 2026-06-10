'use client';

import { useEffect, type RefObject } from 'react';

/**
 * A hook that focuses a given input element when the '/' key is pressed,
 * or Cmd+K / Ctrl+K. It also blurs the input when 'Escape' is pressed.
 */
export function useSearchShortcut(inputRef: RefObject<HTMLInputElement>) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isInputFocused =
        ['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName || '') ||
        (document.activeElement as HTMLElement)?.isContentEditable;

      // Focus shortcut: '/' or Cmd+K or Ctrl+K
      if (
        (e.key === '/' || ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k')) &&
        !isInputFocused
      ) {
        e.preventDefault();
        inputRef.current?.focus();
        return;
      }

      // Blur shortcut: Escape (only if focused on our input)
      if (e.key === 'Escape' && document.activeElement === inputRef.current) {
        inputRef.current?.blur();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [inputRef]);
}
