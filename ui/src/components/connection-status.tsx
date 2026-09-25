'use client';

import { useHealthCheck } from '@/lib/health';

function StatusDot({ color, pulse }: { color: string; pulse?: boolean }) {
  return (
    <span
      className={`inline-block h-2 w-2 rounded-full ${color} ${pulse ? 'animate-pulse' : ''}`}
      aria-hidden="true"
    />
  );
}

export function ConnectionStatus() {
  const { status, checking, isMockMode } = useHealthCheck();

  const baseFocusClasses =
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2';

  if (isMockMode) {
    return (
      <span
        tabIndex={0}
        className={`inline-flex items-center gap-1.5 rounded-full bg-yellow-50 px-2.5 py-0.5 text-xs font-medium text-yellow-700 border border-yellow-200 cursor-help ${baseFocusClasses}`}
        data-testid="connection-status"
        aria-live="polite"
        title="Running in mock API mode (MSW)"
      >
        <StatusDot color="bg-yellow-400" />
        Mock
      </span>
    );
  }

  if (!status && checking) {
    return (
      <span
        tabIndex={0}
        className={`inline-flex items-center gap-1.5 rounded-full bg-gray-50 px-2.5 py-0.5 text-xs font-medium text-gray-500 border border-gray-200 cursor-help ${baseFocusClasses}`}
        data-testid="connection-status"
        aria-live="polite"
        title="Checking backend service health..."
      >
        <StatusDot color="bg-gray-400" pulse />
        Checking...
      </span>
    );
  }

  if (!status) return null;

  if (status.allHealthy) {
    const formattedTime = new Date(status.checkedAt).toLocaleTimeString('en-US', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    return (
      <span
        tabIndex={0}
        role="status"
        className={`inline-flex items-center gap-1.5 rounded-full bg-green-50 px-2.5 py-0.5 text-xs font-medium text-green-700 border border-green-200 cursor-help ${baseFocusClasses}`}
        data-testid="connection-status"
        aria-live="polite"
        title={`All backend services healthy. Last checked: ${formattedTime}`}
      >
        <StatusDot color="bg-green-500" />
        Connected
      </span>
    );
  }

  const unhealthyNames = status.services
    .filter((s) => !s.healthy)
    .map((s) => `${s.name}: ${s.error || 'unreachable'}`)
    .join(', ');

  return (
    <span
      tabIndex={0}
      className={`inline-flex items-center gap-1.5 rounded-full bg-red-50 px-2.5 py-0.5 text-xs font-medium text-red-700 border border-red-200 cursor-help ${baseFocusClasses}`}
      data-testid="connection-status"
      aria-live="polite"
      title={`Disconnected services: ${unhealthyNames}`}
    >
      <StatusDot color="bg-red-500" />
      Disconnected
    </span>
  );
}
