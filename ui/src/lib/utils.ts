import type { ExperimentState, ExperimentType } from './types';

export interface StateConfig {
  label: string;
  dotColor: string;
  bgColor: string;
  textColor: string;
  animate: boolean;
  italic: boolean;
}

export const STATE_CONFIG: Record<ExperimentState, StateConfig> = {
  DRAFT: {
    label: 'Draft',
    dotColor: 'bg-gray-500',
    bgColor: 'bg-gray-100',
    textColor: 'text-gray-700',
    animate: false,
    italic: false,
  },
  STARTING: {
    label: 'Starting',
    dotColor: 'bg-yellow-500',
    bgColor: 'bg-yellow-100',
    textColor: 'text-yellow-800',
    animate: true,
    italic: false,
  },
  RUNNING: {
    label: 'Running',
    dotColor: 'bg-green-500',
    bgColor: 'bg-green-100',
    textColor: 'text-green-800',
    animate: false,
    italic: false,
  },
  CONCLUDING: {
    label: 'Concluding',
    dotColor: 'bg-orange-500',
    bgColor: 'bg-orange-100',
    textColor: 'text-orange-800',
    animate: true,
    italic: false,
  },
  CONCLUDED: {
    label: 'Concluded',
    dotColor: 'bg-blue-500',
    bgColor: 'bg-blue-100',
    textColor: 'text-blue-700',
    animate: false,
    italic: false,
  },
  ARCHIVED: {
    label: 'Archived',
    dotColor: 'bg-gray-400',
    bgColor: 'bg-gray-50',
    textColor: 'text-gray-500',
    animate: false,
    italic: true,
  },
};

export const TYPE_LABELS: Record<ExperimentType, string> = {
  AB: 'A/B Test',
  MULTIVARIATE: 'Multivariate',
  INTERLEAVING: 'Interleaving',
  SESSION_LEVEL: 'Session-Level',
  PLAYBACK_QOE: 'Playback QoE',
  MAB: 'Multi-Armed Bandit',
  CONTEXTUAL_BANDIT: 'Contextual Bandit',
  CUMULATIVE_HOLDOUT: 'Cumulative Holdout',
};

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

export function formatPercent(fraction: number): string {
  return `${(fraction * 100).toFixed(1)}%`;
}

export function truncateJson(json: string, maxLength = 60): string {
  if (json.length <= maxLength) return json;
  return json.slice(0, maxLength) + '…';
}
