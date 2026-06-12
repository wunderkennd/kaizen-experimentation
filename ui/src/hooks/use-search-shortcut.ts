'use client';

import { useEffect, type RefObject } from 'react';

/**
 * A hook that focuses a given input element when the '/' key is pressed,
 * unless the user is already typing in an input, textarea, or contenteditable element.
 */
export function useSearchShortcut(inputRef: RefObject<HTMLInputElement>) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isSearchShortcut =
        (e.key === '/' &&
          !['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName || '') &&
          !(document.activeElement as HTMLElement)?.isContentEditable) ||
        ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k');

      if (isSearchShortcut) {
        e.preventDefault();
        inputRef.current?.focus();
      }

      if (e.key === 'Escape' && document.activeElement === inputRef.current) {
        inputRef.current?.blur();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [inputRef]);
}
