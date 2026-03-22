'use client';

import type { AuditAction } from '@/lib/types';

const ACTION_CONFIG: Record<AuditAction, { label: string; classes: string }> = {
  CREATED: { label: 'Created', classes: 'bg-green-100 text-green-800' },
  UPDATED: { label: 'Updated', classes: 'bg-purple-100 text-purple-800' },
  STARTED: { label: 'Started', classes: 'bg-blue-100 text-blue-800' },
  PAUSED: { label: 'Paused', classes: 'bg-yellow-100 text-yellow-800' },
  RESUMED: { label: 'Resumed', classes: 'bg-teal-100 text-teal-800' },
  CONCLUDED: { label: 'Concluded', classes: 'bg-indigo-100 text-indigo-800' },
  ARCHIVED: { label: 'Archived', classes: 'bg-gray-100 text-gray-800' },
  GUARDRAIL_BREACH: { label: 'Guardrail Breach', classes: 'bg-red-100 text-red-800' },
  CONFIG_CHANGED: { label: 'Config Changed', classes: 'bg-orange-100 text-orange-800' },
};

interface AuditActionBadgeProps {
  action: AuditAction;
}

export function AuditActionBadge({ action }: AuditActionBadgeProps) {
  const config = ACTION_CONFIG[action] || { label: action, classes: 'bg-gray-100 text-gray-800' };
  return (
    <span
      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${config.classes}`}
      data-testid={`action-badge-${action}`}
    >
      {config.label}
    </span>
  );
}
