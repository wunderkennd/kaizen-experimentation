import { render, fireEvent } from '@testing-library/react';
import { useRef } from 'react';
import { describe, it, expect, vi } from 'vitest';
import { useSearchShortcut } from '../../hooks/use-search-shortcut';

function TestComponent() {
  const inputRef = useRef<HTMLInputElement>(null);
  useSearchShortcut(inputRef);
  return <input data-testid="search-input" ref={inputRef} />;
}

describe('useSearchShortcut', () => {
  it('focuses the input when "/" is pressed', () => {
    const { getByTestId } = render(<TestComponent />);
    const input = getByTestId('search-input');

    expect(document.activeElement).not.toBe(input);

    fireEvent.keyDown(window, { key: '/' });
    expect(document.activeElement).toBe(input);
  });

  it('focuses the input when "Cmd+K" is pressed', () => {
    const { getByTestId } = render(<TestComponent />);
    const input = getByTestId('search-input');

    fireEvent.keyDown(window, { key: 'k', metaKey: true });
    expect(document.activeElement).toBe(input);
  });

  it('focuses the input when "Ctrl+K" is pressed', () => {
    const { getByTestId } = render(<TestComponent />);
    const input = getByTestId('search-input');

    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    expect(document.activeElement).toBe(input);
  });

  it('blurs the input when "Escape" is pressed', () => {
    const { getByTestId } = render(<TestComponent />);
    const input = getByTestId('search-input');

    input.focus();
    expect(document.activeElement).toBe(input);

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(document.activeElement).not.toBe(input);
  });

  it('does not focus the input when already in an input', () => {
    const { getByTestId } = render(
      <div>
        <TestComponent />
        <input data-testid="other-input" />
      </div>
    );
    const searchInput = getByTestId('search-input');
    const otherInput = getByTestId('other-input');

    otherInput.focus();
    expect(document.activeElement).toBe(otherInput);

    fireEvent.keyDown(window, { key: '/' });
    expect(document.activeElement).toBe(otherInput);
    expect(document.activeElement).not.toBe(searchInput);
  });
});
