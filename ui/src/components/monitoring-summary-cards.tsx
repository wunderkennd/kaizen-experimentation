'use client';

import type { Experiment, ExperimentState } from '@/lib/types';
import { STATE_CONFIG } from '@/lib/utils';

interface MonitoringSummaryCardsProps {
  experiments: Experiment[];
}

const ALL_STATES: ExperimentState[] = [
  'RUNNING',
  'STARTING',
  'DRAFT',
  'CONCLUDING',
  'CONCLUDED',
  'ARCHIVED',
];

export function MonitoringSummaryCards({ experiments }: MonitoringSummaryCardsProps) {
  const counts: Record<string, number> = {};
  for (const state of ALL_STATES) {
    counts[state] = 0;
  }
  for (const exp of experiments) {
    counts[exp.state] = (counts[exp.state] || 0) + 1;
  }

  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-6" data-testid="summary-cards">
      {ALL_STATES.map((state) => {
        const config = STATE_CONFIG[state];
        return (
          <div
            key={state}
            className={`rounded-lg border p-4 ${config.bgColor}`}
            data-testid={`summary-card-${state}`}
          >
            <div className="flex items-center gap-2">
              <span
                className={`inline-block h-2.5 w-2.5 rounded-full ${config.dotColor} ${config.animate ? 'animate-pulse' : ''}`}
                aria-hidden="true"
              />
              <span className={`text-sm font-medium ${config.textColor}`}>
                {config.label}
              </span>
            </div>
            <p className={`mt-2 text-2xl font-bold ${config.textColor}`} data-testid={`count-${state}`}>
              {counts[state]}
            </p>
          </div>
        );
      })}
    </div>
  );
}
