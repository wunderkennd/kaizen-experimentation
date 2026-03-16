import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ErrorBoundary } from '@/components/error-boundary';

// A component that throws on render
function ThrowOnRender({ shouldThrow }: { shouldThrow: boolean }) {
  if (shouldThrow) {
    throw new Error('Test render crash');
  }
  return <div data-testid="child-content">Everything is fine</div>;
}

// Suppress console.error from React error boundary internals during tests
const originalConsoleError = console.error;
beforeEach(() => {
  console.error = (...args: unknown[]) => {
    const msg = typeof args[0] === 'string' ? args[0] : '';
    if (msg.includes('Error: Uncaught') || msg.includes('The above error occurred')) return;
    originalConsoleError(...args);
  };
});
afterEach(() => {
  console.error = originalConsoleError;
});

describe('ErrorBoundary', () => {
  it('renders children when no error occurs', () => {
    render(
      <ErrorBoundary>
        <ThrowOnRender shouldThrow={false} />
      </ErrorBoundary>,
    );

    expect(screen.getByTestId('child-content')).toBeInTheDocument();
    expect(screen.getByText('Everything is fine')).toBeInTheDocument();
  });

  it('shows default fallback when a child throws', () => {
    render(
      <ErrorBoundary>
        <ThrowOnRender shouldThrow={true} />
      </ErrorBoundary>,
    );

    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Test render crash')).toBeInTheDocument();
    expect(screen.queryByTestId('child-content')).not.toBeInTheDocument();
  });

  it('shows custom fallback when provided', () => {
    render(
      <ErrorBoundary fallback={<div data-testid="custom-fallback">Custom error UI</div>}>
        <ThrowOnRender shouldThrow={true} />
      </ErrorBoundary>,
    );

    expect(screen.getByTestId('custom-fallback')).toBeInTheDocument();
    expect(screen.getByText('Custom error UI')).toBeInTheDocument();
    expect(screen.queryByText('Something went wrong')).not.toBeInTheDocument();
  });

  it('calls onError callback when a child throws', () => {
    const onError = vi.fn();

    render(
      <ErrorBoundary onError={onError}>
        <ThrowOnRender shouldThrow={true} />
      </ErrorBoundary>,
    );

    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'Test render crash' }),
      expect.objectContaining({ componentStack: expect.any(String) }),
    );
  });

  it('recovers when "Try again" is clicked', async () => {
    const user = userEvent.setup();

    // Use a stateful wrapper to toggle the error
    let shouldThrow = true;
    function ToggleChild() {
      if (shouldThrow) throw new Error('Recoverable crash');
      return <div data-testid="recovered">Recovered!</div>;
    }

    const { rerender } = render(
      <ErrorBoundary>
        <ToggleChild />
      </ErrorBoundary>,
    );

    expect(screen.getByText('Something went wrong')).toBeInTheDocument();

    // Fix the error condition before clicking retry
    shouldThrow = false;

    await user.click(screen.getByTestId('error-boundary-retry'));

    // After reset, ErrorBoundary re-renders children
    // Need to rerender since the state reset triggers a re-render
    rerender(
      <ErrorBoundary>
        <ToggleChild />
      </ErrorBoundary>,
    );

    expect(screen.getByTestId('recovered')).toBeInTheDocument();
    expect(screen.queryByText('Something went wrong')).not.toBeInTheDocument();
  });

  it('has role="alert" for accessibility', () => {
    render(
      <ErrorBoundary>
        <ThrowOnRender shouldThrow={true} />
      </ErrorBoundary>,
    );

    expect(screen.getByRole('alert')).toBeInTheDocument();
  });

  it('shows "Try again" button in default fallback', () => {
    render(
      <ErrorBoundary>
        <ThrowOnRender shouldThrow={true} />
      </ErrorBoundary>,
    );

    const retryButton = screen.getByTestId('error-boundary-retry');
    expect(retryButton).toBeInTheDocument();
    expect(retryButton).toHaveTextContent('Try again');
  });
});
