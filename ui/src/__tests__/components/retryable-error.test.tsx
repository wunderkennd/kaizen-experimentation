import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { RetryableError } from '@/components/retryable-error';

describe('RetryableError', () => {
  it('renders message, alert role, and default context header', () => {
    render(<RetryableError message="Network timeout" onRetry={() => {}} />);

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Network timeout')).toBeInTheDocument();
    expect(screen.getByTestId('retry-button')).toHaveTextContent('Retry');
  });

  it('renders custom context header when context prop is provided', () => {
    render(
      <RetryableError
        message="500 Internal Server Error"
        onRetry={() => {}}
        context="metric definitions"
      />
    );

    expect(screen.getByText('Failed to load metric definitions')).toBeInTheDocument();
    expect(screen.getByText('500 Internal Server Error')).toBeInTheDocument();
  });

  it('displays loading state and disables button during async retry execution', async () => {
    let resolveRetry: () => void = () => {};
    const asyncOnRetry = vi.fn().mockImplementation(() => {
      return new Promise<void>((resolve) => {
        resolveRetry = resolve;
      });
    });

    render(<RetryableError message="Failed request" onRetry={asyncOnRetry} />);

    const button = screen.getByTestId('retry-button');
    expect(button).not.toBeDisabled();
    expect(button).toHaveTextContent('Retry');

    fireEvent.click(button);

    // Should immediately show loading spinner & "Retrying..." text, and be disabled
    expect(asyncOnRetry).toHaveBeenCalledTimes(1);
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent('Retrying...');

    // Resolve the async retry handler
    resolveRetry();

    await waitFor(() => {
      expect(button).not.toBeDisabled();
      expect(button).toHaveTextContent('Retry');
    });
  });

  it('restores button state even if async onRetry throws an error', async () => {
    let rejectRetry: (reason?: unknown) => void = () => {};
    const failingAsyncOnRetry = vi.fn().mockImplementation(() => {
      return new Promise<void>((_, reject) => {
        rejectRetry = reject;
      });
    });

    render(<RetryableError message="Failed request" onRetry={failingAsyncOnRetry} />);

    const button = screen.getByTestId('retry-button');
    fireEvent.click(button);

    expect(button).toBeDisabled();
    expect(button).toHaveTextContent('Retrying...');

    // Reject the async operation
    rejectRetry(new Error('Retry failed'));

    await waitFor(() => {
      expect(button).not.toBeDisabled();
      expect(button).toHaveTextContent('Retry');
    });
  });
});
