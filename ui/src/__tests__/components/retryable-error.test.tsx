import { render, screen, act, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { RetryableError } from '@/components/retryable-error';

describe('RetryableError', () => {
  it('renders default error header and message', () => {
    render(<RetryableError message="Network error occurred" onRetry={() => {}} />);

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Network error occurred')).toBeInTheDocument();
    expect(screen.getByTestId('retry-button')).toHaveTextContent('Retry');
  });

  it('renders context-specific header when context prop is provided', () => {
    render(
      <RetryableError
        message="500 Internal Server Error"
        context="experiment results"
        onRetry={() => {}}
      />
    );

    expect(screen.getByText('Failed to load experiment results')).toBeInTheDocument();
    expect(screen.getByText('500 Internal Server Error')).toBeInTheDocument();
  });

  it('handles async onRetry with loading spinner and disabled state', async () => {
    let resolvePromise!: () => void;
    const asyncOnRetry = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolvePromise = resolve;
        })
    );

    render(<RetryableError message="Failed to fetch" onRetry={asyncOnRetry} />);

    const button = screen.getByTestId('retry-button');
    expect(button).not.toBeDisabled();
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();

    // Click retry
    fireEvent.click(button);

    // Should show loading spinner and "Retrying..." text, and be disabled
    expect(asyncOnRetry).toHaveBeenCalledTimes(1);
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent('Retrying...');
    expect(screen.getByTestId('retry-spinner')).toBeInTheDocument();

    // Resolve async retry operation
    await act(async () => {
      resolvePromise();
    });

    // Should return to normal state
    expect(button).not.toBeDisabled();
    expect(button).toHaveTextContent('Retry');
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();
  });
});
