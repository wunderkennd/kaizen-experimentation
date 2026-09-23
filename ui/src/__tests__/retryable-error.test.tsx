import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { RetryableError } from '../components/retryable-error';

describe('RetryableError', () => {
  it('renders error message and default context header', () => {
    render(<RetryableError message="Network timeout" onRetry={() => {}} />);

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Network timeout')).toBeInTheDocument();
    expect(screen.getByTestId('retry-button')).toHaveTextContent('Retry');
  });

  it('renders custom context header', () => {
    render(<RetryableError message="500 Internal Error" context="metric definitions" onRetry={() => {}} />);

    expect(screen.getByText('Failed to load metric definitions')).toBeInTheDocument();
  });

  it('calls synchronous onRetry on button click', async () => {
    const onRetry = vi.fn();
    const user = userEvent.setup();

    render(<RetryableError message="Error" onRetry={onRetry} />);

    await user.click(screen.getByTestId('retry-button'));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('handles asynchronous onRetry with loading state and spinner', async () => {
    let resolveRetry!: () => void;
    const asyncRetry = vi.fn().mockImplementation(() => {
      return new Promise<void>((resolve) => {
        resolveRetry = resolve;
      });
    });

    const user = userEvent.setup();
    render(<RetryableError message="Error" onRetry={asyncRetry} />);

    const button = screen.getByTestId('retry-button');
    expect(button).not.toBeDisabled();
    expect(button).toHaveTextContent('Retry');

    await user.click(button);

    // During async execution, button should be disabled and show "Retrying..."
    expect(button).toBeDisabled();
    expect(button).toHaveTextContent('Retrying...');

    // Resolve the promise
    resolveRetry();

    await waitFor(() => {
      expect(button).not.toBeDisabled();
      expect(button).toHaveTextContent('Retry');
    });
  });

  it('resets retrying state even if onRetry throws an error', async () => {
    const asyncRetry = vi.fn().mockRejectedValue(new Error('Retry failed'));
    const user = userEvent.setup();

    render(<RetryableError message="Error" onRetry={asyncRetry} />);

    const button = screen.getByTestId('retry-button');
    await user.click(button);

    await waitFor(() => {
      expect(button).not.toBeDisabled();
      expect(button).toHaveTextContent('Retry');
    });
  });
});
