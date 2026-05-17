/**
 * Tests for ADR-027 (TOST equivalence testing) M6 results view.
 *
 * Coverage:
 *   - deriveEquivalenceStatus: green / yellow / red verdict geometry
 *   - EquivalenceResultBadge: label + status per verdict, pending fallback
 *   - EquivalenceCiPlot: renders point estimate / CI / δ / TOST p readouts
 *   - EquivalencePowerIndicator: adequate / underpowered / pending states
 */

import React from 'react';
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { EquivalenceResult } from '@/lib/types';

// Recharts uses ResizeObserver / SVG layout unavailable in JSDOM.
vi.mock('recharts', async () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div data-testid="recharts-wrapper">{children}</div>
  );
  const Noop = () => null;
  return {
    ResponsiveContainer: Passthrough,
    ComposedChart: Passthrough,
    Scatter: Passthrough,
    ErrorBar: Noop,
    XAxis: Noop,
    YAxis: Noop,
    CartesianGrid: Noop,
    ReferenceLine: Noop,
    ReferenceArea: Noop,
    Tooltip: Noop,
  };
});

import {
  EquivalenceResultBadge,
  deriveEquivalenceStatus,
} from '@/components/equivalence-result-badge';
import { EquivalenceCiPlot } from '@/components/charts/equivalence-ci-plot';
import { EquivalencePowerIndicator } from '@/components/equivalence-power-indicator';

// --- Fixtures (effect = treatment − control, margin ±δ) ---

const EQUIVALENT: EquivalenceResult = {
  pointEstimate: 0.005,
  stdError: 0.012,
  df: 980,
  pLower: 0.004,
  pUpper: 0.009,
  pTost: 0.009,
  ciLower: -0.02,
  ciUpper: 0.03,
  equivalent: true,
  delta: 0.05,
  controlMean: 1.2,
  treatmentMean: 1.205,
  achievedPower: 0.92,
};

const INCONCLUSIVE: EquivalenceResult = {
  ...EQUIVALENT,
  pTost: 0.21,
  ciLower: -0.02,
  ciUpper: 0.08, // straddles the upper margin boundary
  equivalent: false,
  achievedPower: 0.55,
};

const NOT_EQUIVALENT: EquivalenceResult = {
  ...EQUIVALENT,
  pointEstimate: 0.11,
  pTost: 0.74,
  ciLower: 0.08, // entire CI lies beyond +δ
  ciUpper: 0.15,
  equivalent: false,
  achievedPower: 0.88,
};

describe('deriveEquivalenceStatus', () => {
  it('returns EQUIVALENT when the Rust equivalent flag is set (CI ⊂ ±δ)', () => {
    expect(deriveEquivalenceStatus(EQUIVALENT)).toBe('EQUIVALENT');
  });

  it('returns NOT_EQUIVALENT when the CI lies entirely beyond the margin', () => {
    expect(deriveEquivalenceStatus(NOT_EQUIVALENT)).toBe('NOT_EQUIVALENT');
  });

  it('returns INCONCLUSIVE when the CI straddles a margin boundary', () => {
    expect(deriveEquivalenceStatus(INCONCLUSIVE)).toBe('INCONCLUSIVE');
  });
});

describe('EquivalenceResultBadge', () => {
  it('renders a green Equivalent badge', () => {
    render(<EquivalenceResultBadge result={EQUIVALENT} />);
    const badge = screen.getByTestId('equivalence-result-badge');
    expect(badge).toHaveAttribute('data-status', 'EQUIVALENT');
    expect(badge).toHaveTextContent('Equivalent');
    expect(badge.className).toContain('bg-green-100');
  });

  it('renders a yellow Inconclusive badge', () => {
    render(<EquivalenceResultBadge result={INCONCLUSIVE} />);
    const badge = screen.getByTestId('equivalence-result-badge');
    expect(badge).toHaveAttribute('data-status', 'INCONCLUSIVE');
    expect(badge).toHaveTextContent('Inconclusive');
    expect(badge.className).toContain('bg-yellow-100');
  });

  it('renders a red Not Equivalent badge', () => {
    render(<EquivalenceResultBadge result={NOT_EQUIVALENT} />);
    const badge = screen.getByTestId('equivalence-result-badge');
    expect(badge).toHaveAttribute('data-status', 'NOT_EQUIVALENT');
    expect(badge).toHaveTextContent('Not Equivalent');
    expect(badge.className).toContain('bg-red-100');
  });

  it('renders a pending badge when no result is available', () => {
    render(<EquivalenceResultBadge />);
    const badge = screen.getByTestId('equivalence-result-badge');
    expect(badge).toHaveAttribute('data-status', 'PENDING');
    expect(badge).toHaveTextContent('Equivalence Pending');
  });
});

describe('EquivalenceCiPlot', () => {
  it('renders the CI, point estimate, margin and TOST p-value readouts', () => {
    render(<EquivalenceCiPlot result={EQUIVALENT} metricId="startup_latency_ms" />);
    expect(screen.getByTestId('equivalence-ci-plot')).toBeInTheDocument();
    expect(screen.getByText(/Equivalence CI — startup_latency_ms/)).toBeInTheDocument();
    expect(screen.getByTestId('equiv-point-estimate')).toHaveTextContent('0.0050');
    expect(screen.getByTestId('equiv-ci')).toHaveTextContent('[-0.0200, 0.0300]');
    expect(screen.getByTestId('equiv-delta')).toHaveTextContent('±0.0500');
    expect(screen.getByTestId('equiv-p-tost')).toHaveTextContent('0.0090');
  });
});

describe('EquivalencePowerIndicator', () => {
  it('renders an adequate power bar when power ≥ target', () => {
    render(<EquivalencePowerIndicator achievedPower={0.92} />);
    const el = screen.getByTestId('equivalence-power-indicator');
    expect(el).toHaveAttribute('data-state', 'adequate');
    expect(screen.getByTestId('equiv-power-value')).toHaveTextContent('92%');
  });

  it('warns when underpowered and surfaces the ~2× sample size guidance', () => {
    render(<EquivalencePowerIndicator achievedPower={0.55} />);
    const el = screen.getByTestId('equivalence-power-indicator');
    expect(el).toHaveAttribute('data-state', 'underpowered');
    expect(screen.getByTestId('equiv-power-value')).toHaveTextContent('55%');
    expect(el).toHaveTextContent(/~2× the sample size/);
  });

  it('renders a pending state when power is not yet available', () => {
    render(<EquivalencePowerIndicator />);
    const el = screen.getByTestId('equivalence-power-indicator');
    expect(el).toHaveAttribute('data-state', 'pending');
  });
});
