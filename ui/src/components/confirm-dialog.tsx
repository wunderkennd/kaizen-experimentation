'use client';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  confirmColor?: 'red' | 'green' | 'blue';
  onConfirm: () => void;
  onCancel: () => void;
  loading?: boolean;
}

const COLOR_MAP = {
  red: 'bg-red-600 hover:bg-red-700 focus-visible:outline-red-600',
  green: 'bg-green-600 hover:bg-green-700 focus-visible:outline-green-600',
  blue: 'bg-blue-600 hover:bg-blue-700 focus-visible:outline-blue-600',
} as const;

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  confirmColor = 'blue',
  onConfirm,
  onCancel,
  loading = false,
}: ConfirmDialogProps) {
  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      data-testid="confirm-dialog"
    >
      <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl">
        <h3 className="text-lg font-semibold text-gray-900">{title}</h3>
        <p className="mt-2 text-sm text-gray-600">{message}</p>
        <div className="mt-4 flex justify-end gap-3">
          <button
            type="button"
            onClick={onCancel}
            disabled={loading}
            className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={loading}
            className={`rounded-md px-3 py-2 text-sm font-medium text-white focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 disabled:opacity-50 ${COLOR_MAP[confirmColor]}`}
          >
            {loading ? 'Processing...' : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
