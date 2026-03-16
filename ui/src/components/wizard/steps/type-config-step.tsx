'use client';

import { useWizard } from '../wizard-context';
import { TYPE_LABELS } from '@/lib/utils';
import { InterleavingConfigForm } from './interleaving-config-form';
import { SessionConfigForm } from './session-config-form';
import { BanditConfigForm } from './bandit-config-form';
import { QoeConfigForm } from './qoe-config-form';

export function TypeConfigStep() {
  const { state } = useWizard();

  return (
    <section>
      <h2 className="mb-4 text-lg font-semibold text-gray-900">
        {TYPE_LABELS[state.type]} Configuration
      </h2>
      {renderConfigForm(state.type)}
    </section>
  );
}

function renderConfigForm(type: string) {
  switch (type) {
    case 'INTERLEAVING':
      return <InterleavingConfigForm />;
    case 'SESSION_LEVEL':
      return <SessionConfigForm />;
    case 'MAB':
    case 'CONTEXTUAL_BANDIT':
      return <BanditConfigForm />;
    case 'PLAYBACK_QOE':
      return <QoeConfigForm />;
    default:
      return (
        <p className="rounded-md bg-gray-50 p-4 text-sm text-gray-600">
          No additional configuration needed for this experiment type.
        </p>
      );
  }
}
