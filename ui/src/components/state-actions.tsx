'use client';

import { useState } from 'react';
import type { ExperimentState } from '@/lib/types';
import type { UserRole } from '@/lib/auth';
import { ROLE_LABELS } from '@/lib/auth';
import { useAuth } from '@/lib/auth-context';
import { ConfirmDialog } from './confirm-dialog';

interface StateActionsProps {
  state: ExperimentState;
  onTransition: (action: 'start' | 'conclude' | 'archive') => Promise<void>;
}

const ACTION_CONFIG = {
  DRAFT: {
    action: 'start' as const,
    label: 'Start Experiment',
    title: 'Start Experiment',
    message: 'This will begin traffic allocation and data collection. This action cannot be undone.',
    confirmLabel: 'Start',
    confirmColor: 'green' as const,
    buttonClass: 'bg-green-600 hover:bg-green-700 text-white',
    requiredRole: 'experimenter' as UserRole,
  },
  RUNNING: {
    action: 'conclude' as const,
    label: 'Conclude Experiment',
    title: 'Conclude Experiment',
    message: 'This will stop traffic allocation and trigger final analysis. Results will be available after analysis completes.',
    confirmLabel: 'Conclude',
    confirmColor: 'blue' as const,
    buttonClass: 'bg-blue-600 hover:bg-blue-700 text-white',
    requiredRole: 'experimenter' as UserRole,
  },
  CONCLUDED: {
    action: 'archive' as const,
    label: 'Archive Experiment',
    title: 'Archive Experiment',
    message: 'This will archive the experiment. Results will remain queryable.',
    confirmLabel: 'Archive',
    confirmColor: 'red' as const,
    buttonClass: 'bg-gray-600 hover:bg-gray-700 text-white',
    requiredRole: 'admin' as UserRole,
  },
} as const;

export function StateActions({ state, onTransition }: StateActionsProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const { canAtLeast, user } = useAuth();

  const config = ACTION_CONFIG[state as keyof typeof ACTION_CONFIG];
  if (!config) return null;

  const allowed = canAtLeast(config.requiredRole);
  const tooltip = allowed
    ? config.label
    : `Requires ${ROLE_LABELS[config.requiredRole]} role (you are ${ROLE_LABELS[user.role]})`;

  const handleConfirm = async () => {
    setLoading(true);
    try {
      await onTransition(config.action);
    } finally {
      setLoading(false);
      setDialogOpen(false);
    }
  };

  return (
    <>
      <button
        type="button"
        onClick={() => allowed && setDialogOpen(true)}
        disabled={!allowed}
        title={tooltip}
        className={`rounded-md px-3 py-2 text-sm font-medium ${config.buttonClass} ${
          !allowed ? 'opacity-50 cursor-not-allowed' : ''
        }`}
      >
        {config.label}
      </button>
      <ConfirmDialog
        open={dialogOpen}
        title={config.title}
        message={config.message}
        confirmLabel={config.confirmLabel}
        confirmColor={config.confirmColor}
        onConfirm={handleConfirm}
        onCancel={() => setDialogOpen(false)}
        loading={loading}
      />
    </>
  );
}
