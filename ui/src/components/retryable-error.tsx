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
      className="flex items-start gap-3 rounded-md border border-red-200 bg-red-50 p-4"
      data-testid="retryable-error"
    >
      <div className="flex-shrink-0">
        <svg
          className="h-5 w-5 text-red-400"
          viewBox="0 0 20 20"
          fill="currentColor"
          aria-hidden="true"
        >
          <path
            fillRule="evenodd"
            d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z"
            clipRule="evenodd"
          />
        </svg>
      </div>
      <div className="flex-1">
        <h3 className="text-sm font-semibold text-red-800">
          {context ? `Failed to load ${context}` : 'Something went wrong'}
        </h3>
        <div className="mt-1 text-sm text-red-700">
          <p>{message}</p>
        </div>
        <div className="mt-3">
          <button
            type="button"
            onClick={onRetry}
            className="rounded-md bg-red-600 px-3 py-2 text-sm font-medium text-white hover:bg-red-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-500 focus-visible:ring-offset-2"
            data-testid="retry-button"
          >
            Retry
          </button>
        </div>
      </div>
    </div>
  );
}
