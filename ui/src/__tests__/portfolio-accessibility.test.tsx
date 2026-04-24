import { render, screen, fireEvent } from '@testing-library/react';
import { ExperimentPortfolioTable } from '../components/experiment-portfolio-table';
import { describe, it, expect, vi } from 'vitest';
import type { PortfolioExperiment } from '../lib/types';

const mockExperiments: PortfolioExperiment[] = [
  {
    experimentId: 'exp-1',
    name: 'Experiment 1',
    effectSize: 0.1,
    variance: 0.01,
    allocatedTrafficPct: 0.5,
    priorityScore: 0.8,
    userSegments: ['all'],
  },
];

describe('ExperimentPortfolioTable Accessibility', () => {
  it('renders sortable headers as buttons', () => {
    render(<ExperimentPortfolioTable experiments={mockExperiments} />);

    const headers = ['Experiment', 'Effect Size', 'Variance', 'Traffic %', 'Priority Score'];
    headers.forEach(headerText => {
      const button = screen.getByRole('button', { name: new RegExp(headerText, 'i') });
      expect(button).toBeDefined();
    });
  });

  it('toggles sort on click', () => {
    render(<ExperimentPortfolioTable experiments={mockExperiments} />);

    const experimentHeader = screen.getByRole('button', { name: /Experiment/i });
    const th = experimentHeader.closest('th');

    expect(th?.getAttribute('aria-sort')).toBe('none');

    fireEvent.click(experimentHeader);
    expect(th?.getAttribute('aria-sort')).toBe('descending');

    fireEvent.click(experimentHeader);
    expect(th?.getAttribute('aria-sort')).toBe('ascending');
  });
});
