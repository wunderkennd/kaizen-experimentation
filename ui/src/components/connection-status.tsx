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

  if (isMockMode) {
    return (
      <span
        className="inline-flex items-center gap-1.5 rounded-full bg-yellow-50 px-2.5 py-0.5 text-xs font-medium text-yellow-700 border border-yellow-200"
        data-testid="connection-status"
        aria-live="polite"
      >
        <StatusDot color="bg-yellow-400" />
        Mock
      </span>
    );
  }

  if (!status && checking) {
    return (
      <span
        className="inline-flex items-center gap-1.5 rounded-full bg-gray-50 px-2.5 py-0.5 text-xs font-medium text-gray-500 border border-gray-200"
        data-testid="connection-status"
        aria-live="polite"
      >
        <StatusDot color="bg-gray-400" pulse />
        Checking...
      </span>
    );
  }

  if (!status) return null;

  if (status.allHealthy) {
    return (
      <span
        className="inline-flex items-center gap-1.5 rounded-full bg-green-50 px-2.5 py-0.5 text-xs font-medium text-green-700 border border-green-200"
        data-testid="connection-status"
        aria-live="polite"
        title={`Last checked: ${status.checkedAt}`}
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
      className="inline-flex items-center gap-1.5 rounded-full bg-red-50 px-2.5 py-0.5 text-xs font-medium text-red-700 border border-red-200"
      data-testid="connection-status"
      aria-live="polite"
      title={unhealthyNames}
    >
      <StatusDot color="bg-red-500" />
      Disconnected
    </span>
  );
}
