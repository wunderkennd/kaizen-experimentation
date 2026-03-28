import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { CopyButton } from '@/components/copy-button';
import { ToastProvider } from '@/lib/toast-context';
import { describe, it, expect, vi } from 'vitest';

// Wrap with ToastProvider since CopyButton uses useToast
const renderWithToast = (ui: React.ReactElement) => {
  return render(<ToastProvider>{ui}</ToastProvider>);
};

describe('CopyButton', () => {
  it('renders correctly with default label', () => {
    renderWithToast(<CopyButton text="test-text" />);
    const button = screen.getByRole('button', { name: /copy to clipboard/i });
    expect(button).toBeInTheDocument();
  });

  it('renders with custom label', () => {
    renderWithToast(<CopyButton text="test-text" label="Custom Copy" />);
    const button = screen.getByRole('button', { name: /custom copy/i });
    expect(button).toBeInTheDocument();
  });

  it('copies text to clipboard and shows success state', async () => {
    const textToCopy = 'secret-id-123';
    renderWithToast(<CopyButton text={textToCopy} />);

    const button = screen.getByRole('button');
    fireEvent.click(button);

    // Verify clipboard call
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(textToCopy);

    // Verify visual feedback (SVG change)
    await waitFor(() => {
      // The checkmark SVG has path d="M5 13l4 4L19 7"
      const checkIcon = screen.getByRole('button').querySelector('path[d="M5 13l4 4L19 7"]');
      expect(checkIcon).toBeInTheDocument();
    });
  });

  it('resets state after timeout', async () => {
    vi.useFakeTimers();
    renderWithToast(<CopyButton text="test" />);

    const button = screen.getByRole('button');
    fireEvent.click(button);

    // Fast-forward 2.5s to ensure all effects/timeouts run
    vi.advanceTimersByTime(2500);

    // Check it reverted back to copy icon
    expect(screen.getByRole('button').querySelector('path[d="M8 5H6a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2v-1M8 5a2 2 0 002 2h2a2 2 0 002-2M8 5a2 2 0 012-2h2a2 2 0 012 2m0 0h2a2 2 0 012 2v3m2 4H10m0 0l3-3m-3 3l3 3"]')).toBeInTheDocument();

    vi.useRealTimers();
  });
});
