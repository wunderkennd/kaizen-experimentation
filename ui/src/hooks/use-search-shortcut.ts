'use client';

import { useEffect, type RefObject } from 'react';

/**
 * A hook that focuses a given input element when the '/', Cmd+K, or Ctrl+K keys are pressed,
 * unless the user is already typing in an input, textarea, or contenteditable element.
 * Also blurs the input when the Escape key is pressed while the input is focused.
 */
export function useSearchShortcut(inputRef: RefObject<HTMLInputElement>) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isSearchShortcut =
        e.key === '/' || ((e.metaKey || e.ctrlKey) && e.key === 'k');

      if (
        isSearchShortcut &&
        !['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName || '') &&
        !(document.activeElement as HTMLElement)?.isContentEditable
      ) {
        e.preventDefault();
        inputRef.current?.focus();
        return;
      }

      if (e.key === 'Escape' && document.activeElement === inputRef.current) {
        inputRef.current?.blur();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [inputRef]);
}
