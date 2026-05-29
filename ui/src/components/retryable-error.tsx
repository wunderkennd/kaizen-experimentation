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
      <div className="flex-shrink-0 pt-0.5">
        <svg
          className="h-5 w-5 text-red-400"
          viewBox="0 0 20 20"
          fill="currentColor"
          aria-hidden="true"
        >
          <path
            fillRule="evenodd"
            d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z"
            clipRule="evenodd"
          />
        </svg>
      </div>
      <div className="flex-1 min-w-0">
        <h3 className="text-sm font-semibold text-red-800">
          {context ? `Failed to load ${context}` : 'Something went wrong'}
        </h3>
        <p className="mt-1 text-sm text-red-700">{message}</p>
        <button
          type="button"
          onClick={onRetry}
          className="mt-3 rounded-md bg-red-600 px-3 py-2 text-sm font-medium text-white hover:bg-red-700 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-500 focus-visible:ring-offset-2"
          data-testid="retry-button"
        >
          Retry
        </button>
      </div>
    </div>
  );
}
