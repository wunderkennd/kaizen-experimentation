import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { RetryableError } from '@/components/retryable-error';

describe('RetryableError component UX & Accessibility', () => {
  it('renders default message and button', () => {
    render(<RetryableError message="Network error" onRetry={() => {}} />);
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Network error')).toBeInTheDocument();
    expect(screen.getByTestId('retry-button')).toHaveTextContent('Retry');
  });

  it('renders with context title when provided', () => {
    render(<RetryableError message="500 Internal Server Error" onRetry={() => {}} context="experiment details" />);
    expect(screen.getByText('Failed to load experiment details')).toBeInTheDocument();
  });

  it('shows loading spinner, disables button, and updates label to Retrying... during async retry', async () => {
    let resolveRetry: () => void = () => {};
    const asyncRetry = vi.fn().mockImplementation(() => new Promise<void>((resolve) => {
      resolveRetry = resolve;
    }));

    render(<RetryableError message="Service unavailable" onRetry={asyncRetry} />);

    const retryButton = screen.getByTestId('retry-button');
    expect(retryButton).not.toBeDisabled();
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();

    await userEvent.click(retryButton);

    expect(asyncRetry).toHaveBeenCalledTimes(1);
    expect(retryButton).toBeDisabled();
    expect(retryButton).toHaveTextContent('Retrying...');
    expect(screen.getByTestId('retry-spinner')).toBeInTheDocument();

    // Resolve the promise
    resolveRetry();

    // Wait for async state update
    await screen.findByText('Retry');
    expect(retryButton).not.toBeDisabled();
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();
  });
});
