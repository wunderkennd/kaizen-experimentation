import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { RetryableError } from '@/components/retryable-error';

describe('RetryableError', () => {
  it('renders title, message, and retry button', () => {
    render(
      <RetryableError
        message="Network error occurred"
        context="metrics list"
        onRetry={() => {}}
      />
    );

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Failed to load metrics list')).toBeInTheDocument();
    expect(screen.getByText('Network error occurred')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });

  it('renders default title when context is omitted', () => {
    render(
      <RetryableError
        message="An unexpected error occurred"
        onRetry={() => {}}
      />
    );

    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
  });

  it('handles synchronous onRetry', async () => {
    const onRetry = vi.fn();
    const user = userEvent.setup();

    render(
      <RetryableError
        message="Sync error"
        onRetry={onRetry}
      />
    );

    const button = screen.getByRole('button', { name: 'Retry' });
    await user.click(button);

    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('manages async loading state during onRetry promise execution', async () => {
    let resolvePromise!: () => void;
    const asyncOnRetry = vi.fn(() => new Promise<void>((resolve) => {
      resolvePromise = resolve;
    }));

    const user = userEvent.setup();

    render(
      <RetryableError
        message="Async error"
        onRetry={asyncOnRetry}
      />
    );

    const button = screen.getByRole('button', { name: 'Retry' });
    expect(button).not.toBeDisabled();
    expect(button).not.toHaveAttribute('aria-busy');

    // Click retry
    await user.click(button);

    // During pending promise: button disabled, aria-busy true, shows spinner and "Retrying..." text
    expect(asyncOnRetry).toHaveBeenCalledTimes(1);
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(screen.getByTestId('retry-spinner')).toBeInTheDocument();
    expect(screen.getByText('Retrying...')).toBeInTheDocument();

    // Resolve the promise
    await act(async () => {
      resolvePromise();
    });

    // After resolution: button re-enabled, aria-busy removed, resets text to "Retry"
    expect(button).not.toBeDisabled();
    expect(button).not.toHaveAttribute('aria-busy');
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();
    expect(screen.getByText('Retry')).toBeInTheDocument();
  });

  it('handles rejected onRetry promises gracefully without breaking state', async () => {
    let rejectPromise!: (err: Error) => void;
    const failingOnRetry = vi.fn(() => new Promise<void>((_, reject) => {
      rejectPromise = reject;
    }));

    const user = userEvent.setup();

    render(
      <RetryableError
        message="Failing error"
        onRetry={failingOnRetry}
      />
    );

    const button = screen.getByRole('button', { name: 'Retry' });
    await user.click(button);

    expect(button).toBeDisabled();

    await act(async () => {
      rejectPromise(new Error('Retry failed again'));
    });

    // Reset button after rejection
    expect(button).not.toBeDisabled();
    expect(screen.getByText('Retry')).toBeInTheDocument();
  });
});
