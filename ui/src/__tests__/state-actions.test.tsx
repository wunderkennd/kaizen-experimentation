import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { StateActions } from '@/components/state-actions';
import type { ExperimentState } from '@/lib/types';

describe('StateActions', () => {
  it('shows "Start Experiment" for DRAFT state', () => {
    render(<StateActions state="DRAFT" onTransition={vi.fn()} />);
    expect(screen.getByText('Start Experiment')).toBeInTheDocument();
  });

  it('shows "Conclude Experiment" for RUNNING state', () => {
    render(<StateActions state="RUNNING" onTransition={vi.fn()} />);
    expect(screen.getByText('Conclude Experiment')).toBeInTheDocument();
  });

  it('shows "Archive Experiment" for CONCLUDED state', () => {
    render(<StateActions state="CONCLUDED" onTransition={vi.fn()} />);
    expect(screen.getByText('Archive Experiment')).toBeInTheDocument();
  });

  it('renders nothing for STARTING state', () => {
    const { container } = render(<StateActions state="STARTING" onTransition={vi.fn()} />);
    expect(container.innerHTML).toBe('');
  });

  it('renders nothing for CONCLUDING state', () => {
    const { container } = render(<StateActions state="CONCLUDING" onTransition={vi.fn()} />);
    expect(container.innerHTML).toBe('');
  });

  it('opens confirmation dialog on click', async () => {
    const user = userEvent.setup();
    render(<StateActions state="DRAFT" onTransition={vi.fn()} />);

    await user.click(screen.getByText('Start Experiment'));
    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();
    expect(screen.getByText('Start')).toBeInTheDocument();
  });

  it('calls onTransition when confirmed', async () => {
    const user = userEvent.setup();
    const onTransition = vi.fn().mockResolvedValue(undefined);
    render(<StateActions state="DRAFT" onTransition={onTransition} />);

    await user.click(screen.getByText('Start Experiment'));
    await user.click(screen.getByText('Start'));

    await waitFor(() => {
      expect(onTransition).toHaveBeenCalledWith('start');
    });
  });

  it('does not call onTransition when cancelled', async () => {
    const user = userEvent.setup();
    const onTransition = vi.fn().mockResolvedValue(undefined);
    render(<StateActions state="DRAFT" onTransition={onTransition} />);

    await user.click(screen.getByText('Start Experiment'));
    await user.click(screen.getByText('Cancel'));

    expect(onTransition).not.toHaveBeenCalled();
  });
});
