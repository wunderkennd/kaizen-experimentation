import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { RetryableError } from '@/components/retryable-error';

describe('RetryableError Component', () => {
  it('renders message, context heading, alert role, and retry button', () => {
    render(
      <RetryableError
        message="Network error occurred"
        context="experiment results"
        onRetry={() => {}}
      />
    );

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Failed to load experiment results')).toBeInTheDocument();
    expect(screen.getByText('Network error occurred')).toBeInTheDocument();

    const button = screen.getByRole('button', { name: /retry/i });
    expect(button).toBeInTheDocument();
    expect(button).toHaveAttribute('type', 'button');
    expect(button).not.toBeDisabled();
  });

  it('renders default title when context is omitted', () => {
    render(
      <RetryableError
        message="Server responded with 500"
        onRetry={() => {}}
      />
    );

    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Server responded with 500')).toBeInTheDocument();
  });

  it('calls synchronous onRetry when clicked', async () => {
    const user = userEvent.setup();
    const onRetry = vi.fn();

    render(
      <RetryableError
        message="Temporary failure"
        onRetry={onRetry}
      />
    );

    const button = screen.getByTestId('retry-button');
    await user.click(button);

    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('shows retrying state, spinner, disabled button, and resets state after async onRetry resolves', async () => {
    const user = userEvent.setup();
    let resolvePromise: () => void;
    const asyncRetry = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolvePromise = resolve;
        })
    );

    render(
      <RetryableError
        message="Connection lost"
        onRetry={asyncRetry}
      />
    );

    const button = screen.getByTestId('retry-button');
    expect(button).toHaveTextContent('Retry');
    expect(button).not.toBeDisabled();
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();

    await user.click(button);

    // Should be in retrying state while promise is pending
    expect(asyncRetry).toHaveBeenCalledTimes(1);
    expect(button).toHaveTextContent('Retrying...');
    expect(button).toBeDisabled();
    expect(screen.getByTestId('retry-spinner')).toBeInTheDocument();

    // Resolve async operation
    resolvePromise!();

    await waitFor(() => {
      expect(button).not.toBeDisabled();
    });

    expect(button).toHaveTextContent('Retry');
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();
  });

  it('resets retrying state even when async onRetry rejects', async () => {
    const user = userEvent.setup();
    let rejectPromise: (err: Error) => void;
    const failingRetry = vi.fn(
      () =>
        new Promise<void>((_, reject) => {
          rejectPromise = reject;
        })
    );

    render(
      <RetryableError
        message="Persistent error"
        onRetry={failingRetry}
      />
    );

    const button = screen.getByTestId('retry-button');
    await user.click(button);

    expect(button).toHaveTextContent('Retrying...');
    expect(button).toBeDisabled();

    // Reject the async operation
    rejectPromise!(new Error('Retry failed again'));

    await waitFor(() => {
      expect(button).not.toBeDisabled();
    });

    expect(button).toHaveTextContent('Retry');
    expect(screen.queryByTestId('retry-spinner')).not.toBeInTheDocument();
  });
});
