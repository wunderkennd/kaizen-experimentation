import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { RetryableError } from '@/components/retryable-error';

describe('RetryableError', () => {
  it('renders default message and button', () => {
    render(<RetryableError message="Network error" onRetry={() => {}} />);
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Network error')).toBeInTheDocument();

    const button = screen.getByTestId('retry-button');
    expect(button).toHaveTextContent('Retry');
    expect(button).toHaveAttribute('aria-label', 'Retry operation');
    expect(button).not.toBeDisabled();
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();
  });

  it('renders context when provided', () => {
    render(<RetryableError message="500 Internal Error" onRetry={() => {}} context="experiments" />);
    expect(screen.getByText('Failed to load experiments')).toBeInTheDocument();
    const button = screen.getByTestId('retry-button');
    expect(button).toHaveAttribute('aria-label', 'Retry experiments');
  });

  it('handles synchronous onRetry click', async () => {
    const onRetry = vi.fn();
    render(<RetryableError message="Error" onRetry={onRetry} />);

    await userEvent.click(screen.getByTestId('retry-button'));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('handles asynchronous onRetry with loading spinner and disabled state', async () => {
    let resolveRetry!: () => void;
    const asyncRetry = vi.fn(() => new Promise<void>((resolve) => {
      resolveRetry = resolve;
    }));

    render(<RetryableError message="Error" onRetry={asyncRetry} context="metrics" />);
    const button = screen.getByTestId('retry-button');

    await userEvent.click(button);

    // During async execution
    expect(asyncRetry).toHaveBeenCalledTimes(1);
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(button).toHaveAttribute('aria-label', 'Retrying metrics');
    expect(button).toHaveTextContent('Retrying...');
    expect(screen.getByTestId('retry-spinner')).toBeInTheDocument();

    // Resolve promise
    resolveRetry();

    // Wait for state reset
    expect(await screen.findByText('Retry')).toBeInTheDocument();
    expect(button).not.toBeDisabled();
    expect(button).toHaveAttribute('aria-busy', 'false');
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();
  });

  it('swallows rejected promise without crashing and resets button state', async () => {
    let rejectRetry!: (err: Error) => void;
    const asyncRetry = vi.fn(() => new Promise<void>((_, reject) => {
      rejectRetry = reject;
    }));

    render(<RetryableError message="Error" onRetry={asyncRetry} />);
    const button = screen.getByTestId('retry-button');

    await userEvent.click(button);
    expect(button).toBeDisabled();

    // Reject promise
    rejectRetry(new Error('Still failing'));

    // Wait for state reset
    expect(await screen.findByText('Retry')).toBeInTheDocument();
    expect(button).not.toBeDisabled();
  });
});
