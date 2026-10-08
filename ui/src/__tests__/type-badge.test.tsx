import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { TypeBadge } from '../components/type-badge';
import { TYPE_LABELS, TYPE_DESCRIPTIONS } from '../lib/utils';
import type { ExperimentType } from '../lib/types';

describe('TypeBadge', () => {
  const experimentTypes: ExperimentType[] = [
    'AB',
    'MULTIVARIATE',
    'INTERLEAVING',
    'SESSION_LEVEL',
    'PLAYBACK_QOE',
    'MAB',
    'CONTEXTUAL_BANDIT',
    'CUMULATIVE_HOLDOUT',
    'SLATE',
    'SWITCHBACK',
    'QUASI_EXPERIMENT',
    'META',
  ];

  experimentTypes.forEach((type) => {
    it(`renders correct label and title tooltip for ${type}`, () => {
      render(<TypeBadge type={type} />);

      const badge = screen.getByTestId('type-badge');
      expect(badge).toBeInTheDocument();
      expect(badge).toHaveTextContent(TYPE_LABELS[type]);
      expect(badge).toHaveAttribute('title', TYPE_DESCRIPTIONS[type]);
      expect(badge).toHaveAttribute('data-type', type);
      expect(badge).toHaveClass('cursor-help');
      expect(badge).toHaveAttribute('tabIndex', '0');
      expect(badge).toHaveAttribute('role', 'note');
      expect(badge).toHaveAttribute('aria-label', `${TYPE_LABELS[type]}: ${TYPE_DESCRIPTIONS[type]}`);
      expect(badge).toHaveClass('focus-visible:ring-2');
    });
  });
});
