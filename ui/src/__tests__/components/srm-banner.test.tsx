import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { SrmBanner } from '@/components/srm-banner';
import type { SrmResult } from '@/lib/types';

describe('SrmBanner', () => {
  it('renders nothing when there is no sample ratio mismatch', () => {
    const srmResult: SrmResult = {
      isMismatch: false,
      pValue: 0.85,
      chiSquared: 0.15,
      observedCounts: { control: 500, treatment: 500 },
      expectedCounts: { control: 500, treatment: 500 },
    };

    const { container } = render(<SrmBanner srmResult={srmResult} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders the alert banner and counts table when a mismatch is detected', () => {
    const srmResult: SrmResult = {
      isMismatch: true,
      pValue: 0.0001,
      chiSquared: 15.42,
      observedCounts: { control: 600, treatment: 400 },
      expectedCounts: { control: 500, treatment: 500 },
    };

    render(<SrmBanner srmResult={srmResult} />);

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Sample Ratio Mismatch Detected')).toBeInTheDocument();
    expect(screen.getByText(/Chi-squared = 15.42/)).toBeInTheDocument();
    expect(screen.getByText(/p-value = < 0.001/)).toBeInTheDocument();

    expect(screen.getByText('control')).toBeInTheDocument();
    expect(screen.getByText('600')).toBeInTheDocument();
    expect(screen.getByText('treatment')).toBeInTheDocument();
    expect(screen.getByText('400')).toBeInTheDocument();
  });
});
