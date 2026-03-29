import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CopyButton } from '@/components/copy-button';
import { ToastProvider } from '@/lib/toast-context';

// Wrap component with necessary providers
const renderWithProviders = (ui: React.ReactElement) => {
  return render(
    <ToastProvider>
      {ui}
    </ToastProvider>
  );
};

describe('CopyButton', () => {
  const testValue = 'test-copy-value';

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders correctly with default label', () => {
    renderWithProviders(<CopyButton value={testValue} />);
    expect(screen.getByRole('button')).toHaveAttribute('aria-label', 'Copy to clipboard');
  });

  it('renders correctly with custom label', () => {
    const customLabel = 'Copy custom ID';
    renderWithProviders(<CopyButton value={testValue} label={customLabel} />);
    expect(screen.getByRole('button')).toHaveAttribute('aria-label', customLabel);
  });

  it('calls navigator.clipboard.writeText when clicked', async () => {
    renderWithProviders(<CopyButton value={testValue} />);
    const button = screen.getByRole('button');

    fireEvent.click(button);

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(testValue);
  });

  it('shows "Copied!" feedback after successful copy', async () => {
    renderWithProviders(<CopyButton value={testValue} />);
    const button = screen.getByRole('button');

    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('Copied!')).toBeInTheDocument();
    });
  });

  it('hides feedback after a delay', async () => {
    vi.useFakeTimers();
    renderWithProviders(<CopyButton value={testValue} />);
    const button = screen.getByRole('button');

    await act(async () => {
      fireEvent.click(button);
    });

    expect(screen.getByText('Copied!')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(3000);
    });

    expect(screen.queryByText('Copied!')).not.toBeInTheDocument();

    vi.useRealTimers();
  });

  it('shows success toast on success', async () => {
    renderWithProviders(<CopyButton value={testValue} successMessage="ID copied!" />);
    const button = screen.getByRole('button');

    await act(async () => {
      fireEvent.click(button);
    });

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(testValue);
  });
});
