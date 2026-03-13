'use client';

interface RetryableErrorProps {
  message: string;
  onRetry: () => void;
  context?: string;
}

export function RetryableError({ message, onRetry, context }: RetryableErrorProps) {
  return (
    <div
      role="alert"
      className="rounded-md bg-red-50 border border-red-200 p-4"
      data-testid="retryable-error"
    >
      <h3 className="text-sm font-semibold text-red-800">
        {context ? `Failed to load ${context}` : 'Something went wrong'}
      </h3>
      <p className="mt-1 text-sm text-red-700">{message}</p>
      <button
        type="button"
        onClick={onRetry}
        className="mt-3 rounded-md bg-red-600 px-3 py-2 text-sm font-medium text-white hover:bg-red-700"
        data-testid="retry-button"
      >
        Retry
      </button>
    </div>
  );
}
