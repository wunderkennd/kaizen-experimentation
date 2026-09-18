import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { RetryableError } from '@/components/retryable-error';

describe('RetryableError', () => {
  it('renders default title and message', () => {
    render(<RetryableError message="Network error occurred" onRetry={() => {}} />);

    expect(screen.getByTestId('retryable-error')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Network error occurred')).toBeInTheDocument();
    expect(screen.getByTestId('retry-button')).toHaveTextContent('Retry');
  });

  it('renders context in title when provided', () => {
    render(<RetryableError message="500 Internal Error" onRetry={() => {}} context="feature flags" />);

    expect(screen.getByText('Failed to load feature flags')).toBeInTheDocument();
  });

  it('handles async retry with loading spinner and disabled state', async () => {
    let resolveRetry: () => void = () => {};
    const onRetry = vi.fn().mockImplementation(() => {
      return new Promise<void>((resolve) => {
        resolveRetry = resolve;
      });
    });

    render(<RetryableError message="Error" onRetry={onRetry} />);

    const retryButton = screen.getByTestId('retry-button');
    expect(retryButton).not.toBeDisabled();
    expect(retryButton).toHaveAttribute('aria-busy', 'false');

    fireEvent.click(retryButton);

    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(retryButton).toBeDisabled();
    expect(retryButton).toHaveAttribute('aria-busy', 'true');
    expect(retryButton).toHaveTextContent('Retrying...');

    resolveRetry();

    await waitFor(() => {
      expect(retryButton).not.toBeDisabled();
      expect(retryButton).toHaveAttribute('aria-busy', 'false');
      expect(retryButton).toHaveTextContent('Retry');
    });
  });
});
