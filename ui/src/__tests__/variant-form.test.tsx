import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { VariantForm } from '@/components/variant-form';
import type { Variant } from '@/lib/types';

const baseVariants: Variant[] = [
  {
    variantId: 'v-ctrl',
    name: 'control',
    trafficFraction: 0.5,
    isControl: true,
    payloadJson: '{"key": "a"}',
  },
  {
    variantId: 'v-treat',
    name: 'treatment',
    trafficFraction: 0.5,
    isControl: false,
    payloadJson: '{"key": "b"}',
  },
];

describe('VariantForm', () => {
  it('renders all variant rows', () => {
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);
    expect(screen.getByDisplayValue('control')).toBeInTheDocument();
    expect(screen.getByDisplayValue('treatment')).toBeInTheDocument();
  });

  it('shows traffic sum as green when sum = 1.0', () => {
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);
    expect(screen.getByText('Total traffic: 100.0%')).toHaveClass('text-green-700');
  });

  it('shows traffic sum as red when sum != 1.0', () => {
    const unbalanced = [
      { ...baseVariants[0], trafficFraction: 0.3 },
      { ...baseVariants[1], trafficFraction: 0.3 },
    ];
    render(<VariantForm variants={unbalanced} experimentType="AB" onSave={vi.fn()} />);
    expect(screen.getByText('Total traffic: 60.0%')).toHaveClass('text-red-700');
  });

  it('can edit variant name', async () => {
    const user = userEvent.setup();
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);

    const nameInput = screen.getByDisplayValue('control');
    await user.clear(nameInput);
    await user.type(nameInput, 'new_name');

    expect(screen.getByDisplayValue('new_name')).toBeInTheDocument();
  });

  it('shows error on blur for empty name', async () => {
    const user = userEvent.setup();
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);

    const nameInput = screen.getByDisplayValue('control');
    await user.clear(nameInput);
    await user.tab(); // blur

    expect(screen.getByText('Variant name is required')).toBeInTheDocument();
  });

  it('shows error on blur for invalid JSON payload', async () => {
    const user = userEvent.setup();
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);

    const payloadInput = screen.getByDisplayValue('{"key": "a"}');
    await user.clear(payloadInput);
    // Use double braces to escape { in user-event
    await user.type(payloadInput, '{{bad');
    await user.tab();

    expect(screen.getByText('Invalid JSON')).toBeInTheDocument();
  });

  it('adds a new variant when "Add Variant" is clicked', async () => {
    const user = userEvent.setup();
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);

    await user.click(screen.getByText('Add Variant'));

    // Should now have 3 rows — the new one has empty name
    const nameInputs = screen.getAllByRole('textbox', { name: /name/i });
    expect(nameInputs).toHaveLength(3);
  });

  it('removes a variant when "Remove" is clicked', async () => {
    const user = userEvent.setup();
    const threeVariants = [
      ...baseVariants,
      {
        variantId: 'v-extra',
        name: 'extra',
        trafficFraction: 0,
        isControl: false,
        payloadJson: '{}',
      },
    ];
    render(<VariantForm variants={threeVariants} experimentType="AB" onSave={vi.fn()} />);

    const removeButtons = screen.getAllByText('Remove');
    await user.click(removeButtons[2]); // remove "extra"

    expect(screen.queryByDisplayValue('extra')).not.toBeInTheDocument();
  });

  it('disables remove button at minimum variant count for AB', () => {
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);
    const removeButtons = screen.getAllByText('Remove');
    removeButtons.forEach((btn) => {
      expect(btn).toBeDisabled();
    });
  });

  it('save button is disabled when not dirty', () => {
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);
    expect(screen.getByText('Save Variants')).toBeDisabled();
  });

  it('calls onSave with updated variants on valid save', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={onSave} />);

    // Make a change to enable save
    const nameInput = screen.getByDisplayValue('control');
    await user.clear(nameInput);
    await user.type(nameInput, 'control_v2');

    await user.click(screen.getByText('Save Variants'));

    await waitFor(() => {
      expect(onSave).toHaveBeenCalledTimes(1);
    });

    const savedVariants = onSave.mock.calls[0][0] as Variant[];
    expect(savedVariants[0].name).toBe('control_v2');
  });

  it('shows banner error when traffic does not sum to 1.0 on save', async () => {
    const user = userEvent.setup();
    const unbalanced = [
      { ...baseVariants[0], trafficFraction: 0.3 },
      { ...baseVariants[1], trafficFraction: 0.3 },
    ];
    render(<VariantForm variants={unbalanced} experimentType="AB" onSave={vi.fn()} />);

    // Make dirty
    const nameInput = screen.getByDisplayValue('control');
    await user.clear(nameInput);
    await user.type(nameInput, 'ctrl');

    await user.click(screen.getByText('Save Variants'));

    expect(screen.getByRole('alert')).toHaveTextContent('Traffic fractions must sum to 100%');
  });

  it('switches control via radio button', async () => {
    const user = userEvent.setup();
    render(<VariantForm variants={baseVariants} experimentType="AB" onSave={vi.fn()} />);

    const radios = screen.getAllByRole('radio');
    expect(radios[0]).toBeChecked(); // control
    expect(radios[1]).not.toBeChecked(); // treatment

    await user.click(radios[1]);

    expect(radios[0]).not.toBeChecked();
    expect(radios[1]).toBeChecked();
  });

  it('distributes traffic evenly when "Distribute Evenly" is clicked', async () => {
    const user = userEvent.setup();
    const threeVariants = [
      ...baseVariants,
      {
        variantId: 'v-3',
        name: 'v3',
        trafficFraction: 0,
        isControl: false,
        payloadJson: '{}',
      },
    ];
    render(<VariantForm variants={threeVariants} experimentType="AB" onSave={vi.fn()} />);

    // Initially: 0.5, 0.5, 0 -> 1.0
    expect(screen.getByText('Total traffic: 100.0%')).toBeInTheDocument();

    await user.click(screen.getByText('Distribute Evenly'));

    // Should be 0.333, 0.333, 0.334
    const trafficInputs = screen.getAllByRole('spinbutton', { name: /traffic/i });
    expect(trafficInputs[0]).toHaveValue(0.333);
    expect(trafficInputs[1]).toHaveValue(0.333);
    expect(trafficInputs[2]).toHaveValue(0.334);
    expect(screen.getByText('Total traffic: 100.0%')).toBeInTheDocument();
  });
});
