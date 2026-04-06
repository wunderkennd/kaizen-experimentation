import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { TreatmentEffectsTable } from '@/components/treatment-effects-table';
import { FdrDecisionBadge } from '@/components/fdr-decision-badge';
import { OptimalAlphaWidget } from '@/components/optimal-alpha-widget';
import type { MetricResult, EValueResult, OnlineFdrState } from '@/lib/types';

// --- Fixtures ---

const METRIC_RESULTS: MetricResult[] = [
  {
    metricId: 'click_through_rate',
    variantId: 'treatment_a',
    controlMean: 0.085,
    treatmentMean: 0.099,
    absoluteEffect: 0.014,
    relativeEffect: 0.1647,
    ciLower: 0.003,
    ciUpper: 0.025,
    pValue: 0.012,
    isSignificant: true,
    cupedAdjustedEffect: 0.013,
    cupedCiLower: 0.004,
    cupedCiUpper: 0.022,
    varianceReductionPct: 0.28,
  },
];

const E_VALUE_REJECTING: EValueResult = {
  eValue: 25.0,
  logEValue: Math.log(25.0),
  impliedLevel: 1 / 25.0,
  reject: true,
  alpha: 0.05,
};

const E_VALUE_NOT_REJECTING: EValueResult = {
  eValue: 12.5,
  logEValue: Math.log(12.5),
  impliedLevel: 1 / 12.5,
  reject: false,
  alpha: 0.05,
};

const FDR_STATE_CONTROLLED: OnlineFdrState = {
  experimentId: '11111111-1111-1111-1111-111111111111',
  alphaWealth: 0.032,
  initialWealth: 0.05,
  numTested: 15,
  numRejected: 3,
  currentFdr: 0.04,
  computedAt: '2026-03-24T10:00:00Z',
};

const FDR_STATE_OVER_BUDGET: OnlineFdrState = {
  experimentId: '11111111-1111-1111-1111-111111111111',
  alphaWealth: 0.001,
  initialWealth: 0.05,
  numTested: 20,
  numRejected: 8,
  currentFdr: 0.08,
  computedAt: '2026-03-24T10:00:00Z',
};

// --- Treatment Effects Table: E-value column ---

describe('TreatmentEffectsTable with e-value', () => {
  it('shows e-value and implied p columns when eValueResult is provided', () => {
    render(
      <TreatmentEffectsTable
        metricResults={METRIC_RESULTS}
        showCuped={false}
        eValueResult={E_VALUE_REJECTING}
      />,
    );

    expect(screen.getByTestId('evalue-header')).toBeInTheDocument();
    expect(screen.getByText('Implied p')).toBeInTheDocument();
  });

  it('does not show e-value column when eValueResult is absent', () => {
    render(
      <TreatmentEffectsTable
        metricResults={METRIC_RESULTS}
        showCuped={false}
      />,
    );

    expect(screen.queryByTestId('evalue-header')).not.toBeInTheDocument();
  });

  it('renders e-value cell with formatted value', () => {
    render(
      <TreatmentEffectsTable
        metricResults={METRIC_RESULTS}
        showCuped={false}
        eValueResult={E_VALUE_REJECTING}
      />,
    );

    const cell = screen.getByTestId('evalue-cell');
    expect(cell).toHaveTextContent('25.0');
  });

  it('highlights e-value cell in red when null is rejected', () => {
    render(
      <TreatmentEffectsTable
        metricResults={METRIC_RESULTS}
        showCuped={false}
        eValueResult={E_VALUE_REJECTING}
      />,
    );

    const cell = screen.getByTestId('evalue-cell');
    const span = cell.querySelector('span');
    expect(span?.className).toContain('text-red-700');
  });

  it('does not highlight e-value cell when null is not rejected', () => {
    render(
      <TreatmentEffectsTable
        metricResults={METRIC_RESULTS}
        showCuped={false}
        eValueResult={E_VALUE_NOT_REJECTING}
      />,
    );

    const cell = screen.getByTestId('evalue-cell');
    const span = cell.querySelector('span');
    expect(span?.className).not.toContain('text-red-700');
  });
});

// --- FDR Decision Badge ---

describe('FdrDecisionBadge', () => {
  it('shows PASS when e-value rejects and FDR is under control', () => {
    render(
      <FdrDecisionBadge
        eValueResult={E_VALUE_REJECTING}
        fdrState={FDR_STATE_CONTROLLED}
      />,
    );

    const badge = screen.getByTestId('fdr-decision-badge');
    expect(badge).toHaveAttribute('data-decision', 'PASS');
    expect(badge).toHaveTextContent('FDR Pass');
  });

  it('shows FAIL when e-value does not reject', () => {
    render(
      <FdrDecisionBadge
        eValueResult={E_VALUE_NOT_REJECTING}
        fdrState={FDR_STATE_CONTROLLED}
      />,
    );

    const badge = screen.getByTestId('fdr-decision-badge');
    expect(badge).toHaveAttribute('data-decision', 'FAIL');
    expect(badge).toHaveTextContent('FDR Fail');
  });

  it('shows FAIL when e-value rejects but FDR is over budget', () => {
    render(
      <FdrDecisionBadge
        eValueResult={E_VALUE_REJECTING}
        fdrState={FDR_STATE_OVER_BUDGET}
      />,
    );

    const badge = screen.getByTestId('fdr-decision-badge');
    expect(badge).toHaveAttribute('data-decision', 'FAIL');
  });

  it('shows PENDING when no e-value result', () => {
    render(<FdrDecisionBadge />);

    const badge = screen.getByTestId('fdr-decision-badge');
    expect(badge).toHaveAttribute('data-decision', 'PENDING');
    expect(badge).toHaveTextContent('FDR Pending');
  });

  it('shows PASS without FDR state when e-value rejects', () => {
    render(<FdrDecisionBadge eValueResult={E_VALUE_REJECTING} />);

    const badge = screen.getByTestId('fdr-decision-badge');
    expect(badge).toHaveAttribute('data-decision', 'PASS');
  });
});

// --- Optimal Alpha Widget ---

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual('@/lib/api');
  return {
    ...actual,
    getOptimalAlpha: vi.fn().mockResolvedValue({
      optimalAlpha: 0.10,
      expectedPortfolioFdr: 0.042,
      computedAt: '2026-03-24T10:00:00Z',
    }),
  };
});

describe('OptimalAlphaWidget', () => {
  it('renders recommended alpha value', async () => {
    render(
      <OptimalAlphaWidget
        experimentId="11111111-1111-1111-1111-111111111111"
        currentAlpha={0.05}
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId('optimal-alpha-widget')).toBeInTheDocument();
    });

    expect(screen.getByText('0.100')).toBeInTheDocument();
    expect(screen.getByText('recommended')).toBeInTheDocument();
  });

  it('shows relaxation suggestion when optimal > current', async () => {
    render(
      <OptimalAlphaWidget
        experimentId="11111111-1111-1111-1111-111111111111"
        currentAlpha={0.05}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Relaxation suggested')).toBeInTheDocument();
    });
  });

  it('displays current alpha for comparison', async () => {
    render(
      <OptimalAlphaWidget
        experimentId="11111111-1111-1111-1111-111111111111"
        currentAlpha={0.05}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('0.050')).toBeInTheDocument();
    });
  });

  it('shows expected portfolio FDR', async () => {
    render(
      <OptimalAlphaWidget
        experimentId="11111111-1111-1111-1111-111111111111"
        currentAlpha={0.05}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('4.2%')).toBeInTheDocument();
    });
  });
});
