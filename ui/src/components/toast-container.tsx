'use client';

import { useToast, type ToastVariant } from '@/lib/toast-context';

const VARIANT_STYLES: Record<ToastVariant, string> = {
  success: 'bg-green-50 border-green-200 text-green-800',
  error: 'bg-red-50 border-red-200 text-red-800',
  info: 'bg-blue-50 border-blue-200 text-blue-800',
  warning: 'bg-yellow-50 border-yellow-200 text-yellow-800',
};

const VARIANT_ICONS: Record<ToastVariant, string> = {
  success: '\u2713',
  error: '\u2717',
  info: '\u2139',
  warning: '\u26A0',
};

export function ToastContainer() {
  const { toasts, removeToast } = useToast();

  if (toasts.length === 0) return null;

  return (
    <div
      className="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col gap-2"
      aria-live="polite"
      aria-label="Notifications"
    >
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`pointer-events-auto flex items-center gap-2 rounded-lg border px-4 py-3 shadow-lg transition-all ${VARIANT_STYLES[toast.variant]}`}
          role="status"
        >
          <span className="text-sm font-medium" aria-hidden="true">{VARIANT_ICONS[toast.variant]}</span>
          <p className="text-sm">{toast.message}</p>
          <button
            type="button"
            onClick={() => removeToast(toast.id)}
            className="ml-2 text-sm opacity-60 hover:opacity-100"
            aria-label="Dismiss notification"
          >
            &times;
          </button>
        </div>
      ))}
    </div>
  );
}
