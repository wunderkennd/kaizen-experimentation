import { renderHook } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useSearchShortcut } from '@/hooks/use-search-shortcut';

describe('useSearchShortcut', () => {
  let inputRef: { current: HTMLInputElement };

  beforeEach(() => {
    inputRef = {
      current: {
        focus: vi.fn(),
        blur: vi.fn(),
      } as unknown as HTMLInputElement,
    };
    // Mock activeElement
    Object.defineProperty(document, 'activeElement', {
      value: document.body,
      writable: true,
      configurable: true,
    });
  });

  it('focuses the input when "/" is pressed', () => {
    renderHook(() => useSearchShortcut(inputRef as any));
    const event = new KeyboardEvent('keydown', { key: '/' });
    window.dispatchEvent(event);
    expect(inputRef.current.focus).toHaveBeenCalled();
  });

  it('focuses the input when "Cmd+K" is pressed', () => {
    renderHook(() => useSearchShortcut(inputRef as any));
    const event = new KeyboardEvent('keydown', { key: 'k', metaKey: true });
    window.dispatchEvent(event);
    expect(inputRef.current.focus).toHaveBeenCalled();
  });

  it('focuses the input when "Ctrl+K" is pressed', () => {
    renderHook(() => useSearchShortcut(inputRef as any));
    const event = new KeyboardEvent('keydown', { key: 'k', ctrlKey: true });
    window.dispatchEvent(event);
    expect(inputRef.current.focus).toHaveBeenCalled();
  });

  it('blurs the input when "Escape" is pressed and input is focused', () => {
    Object.defineProperty(document, 'activeElement', {
      value: inputRef.current,
      writable: true,
      configurable: true,
    });
    renderHook(() => useSearchShortcut(inputRef as any));
    const event = new KeyboardEvent('keydown', { key: 'Escape' });
    window.dispatchEvent(event);
    expect(inputRef.current.blur).toHaveBeenCalled();
  });

  it('does not focus when "/" is pressed and user is in an input', () => {
    const activeInput = document.createElement('input');
    Object.defineProperty(document, 'activeElement', {
      value: activeInput,
      writable: true,
      configurable: true,
    });
    renderHook(() => useSearchShortcut(inputRef as any));
    const event = new KeyboardEvent('keydown', { key: '/' });
    window.dispatchEvent(event);
    expect(inputRef.current.focus).not.toHaveBeenCalled();
  });
});
