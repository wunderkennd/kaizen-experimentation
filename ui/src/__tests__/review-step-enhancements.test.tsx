import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { ReviewStep } from '@/components/wizard/steps/review-step';
import { WizardProvider } from '@/components/wizard/wizard-context';
import { ToastProvider } from '@/lib/toast-context';

// Mock navigator.clipboard
Object.defineProperty(navigator, 'clipboard', {
  value: {
    writeText: vi.fn().mockResolvedValue(undefined),
  },
});

const mockInitialState = {
  step: 4, // Review step
  name: 'Test Experiment',
  ownerEmail: 'test@example.com',
  type: 'AB' as const,
  layerId: 'test-layer-id',
  description: 'Test description',
  variants: [
    { variantId: 'v1', name: 'Control', trafficFraction: 0.5, isControl: true, payloadJson: '{}' },
    { variantId: 'v2', name: 'Treatment', trafficFraction: 0.5, isControl: false, payloadJson: '{}' },
  ],
  primaryMetricId: 'test-metric-id',
  secondaryMetricsInput: '',
  guardrails: [],
  guardrailAction: 'ALERT_ONLY' as const,
  isCumulativeHoldout: false,
  enableSequential: false,
  sequentialMethod: 'ALWAYS_VALID' as const,
  plannedLooks: 1,
  overallAlpha: 0.05,
  interleavingConfig: { method: 'TEAM_DRAFT', algorithmIds: [], creditAssignment: 'REWARD', creditMetricEvent: '', maxListSize: 10 },
  sessionConfig: { sessionIdAttribute: 'session_id', allowCrossSessionVariation: false, minSessionsPerUser: 1 },
  banditExperimentConfig: { algorithm: 'EPSILON_GREEDY', rewardMetricId: 'reward-metric-id', contextFeatureKeys: [], minExplorationFraction: 0.1, warmupObservations: 100 },
  qoeConfig: { qoeMetrics: [], deviceFilter: '' },
  targetingRuleId: 'test-targeting-rule',
};

describe('ReviewStep Enhancements', () => {
  it('renders technical identifiers with code styling and copy buttons', () => {
    render(
      <ToastProvider>
        <WizardProvider initialState={mockInitialState}>
          <ReviewStep />
        </WizardProvider>
      </ToastProvider>
    );

    // Check Layer ID
    const layerIdValue = screen.getByText('test-layer-id');
    expect(layerIdValue.tagName).toBe('CODE');
    expect(screen.getByLabelText('Copy Layer')).toBeInTheDocument();

    // Check Targeting Rule ID
    const targetingRuleValue = screen.getByText('test-targeting-rule');
    expect(targetingRuleValue.tagName).toBe('CODE');
    expect(screen.getByLabelText('Copy Targeting Rule')).toBeInTheDocument();

    // Check Primary Metric
    const primaryMetricValue = screen.getByText('test-metric-id');
    expect(primaryMetricValue.tagName).toBe('CODE');
    expect(screen.getByLabelText('Copy Primary Metric')).toBeInTheDocument();
  });

  it('renders session-specific technical identifiers for SESSION_LEVEL experiments', () => {
    render(
      <ToastProvider>
        <WizardProvider initialState={{ ...mockInitialState, type: 'SESSION_LEVEL' }}>
          <ReviewStep />
        </WizardProvider>
      </ToastProvider>
    );

    const sessionIdValue = screen.getByText('session_id');
    expect(sessionIdValue.tagName).toBe('CODE');
    expect(screen.getByLabelText('Copy Session ID Attribute')).toBeInTheDocument();
  });

  it('renders bandit-specific technical identifiers for MAB experiments', () => {
    render(
      <ToastProvider>
        <WizardProvider initialState={{ ...mockInitialState, type: 'MAB' }}>
          <ReviewStep />
        </WizardProvider>
      </ToastProvider>
    );

    const rewardMetricValue = screen.getByText('reward-metric-id');
    expect(rewardMetricValue.tagName).toBe('CODE');
    expect(screen.getByLabelText('Copy Reward Metric')).toBeInTheDocument();
  });
});
