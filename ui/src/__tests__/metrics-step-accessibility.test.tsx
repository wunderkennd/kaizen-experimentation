import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect } from 'vitest';
import { MetricsStep } from '@/components/wizard/steps/metrics-step';
import { WizardProvider } from '@/components/wizard/wizard-context';

describe('MetricsStep Accessibility', () => {
  it('renders Add Guardrail button with accessible focus-visible styles', () => {
    render(
      <WizardProvider>
        <MetricsStep />
      </WizardProvider>
    );

    const addBtn = screen.getByRole('button', { name: 'Add Guardrail' });
    expect(addBtn).toBeInTheDocument();
    expect(addBtn).toHaveClass('focus-visible:ring-2', 'focus-visible:ring-indigo-500');
  });

  it('renders Remove Guardrail button with descriptive aria-label and red focus ring styles', async () => {
    const user = userEvent.setup();
    render(
      <WizardProvider>
        <MetricsStep />
      </WizardProvider>
    );

    const addBtn = screen.getByRole('button', { name: 'Add Guardrail' });
    await user.click(addBtn);

    // Initial unnamed guardrail defaults to index label
    const removeBtn = screen.getByRole('button', { name: 'Remove guardrail 1' });
    expect(removeBtn).toBeInTheDocument();
    expect(removeBtn).toHaveClass('focus-visible:ring-2', 'focus-visible:ring-red-500');

    // After typing metric ID, label updates dynamically
    const metricInput = screen.getByLabelText('Guardrail 1 metric');
    await user.type(metricInput, 'error_rate');

    const updatedRemoveBtn = screen.getByRole('button', { name: 'Remove guardrail error_rate' });
    expect(updatedRemoveBtn).toBeInTheDocument();
  });
});
