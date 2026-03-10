import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { StateActions } from '@/components/state-actions';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';

const adminUser: AuthUser = { email: 'admin@test.com', role: 'admin' };
const experimenterUser: AuthUser = { email: 'exp@test.com', role: 'experimenter' };
const viewerUser: AuthUser = { email: 'viewer@test.com', role: 'viewer' };
const analystUser: AuthUser = { email: 'analyst@test.com', role: 'analyst' };

function renderWithAuth(ui: React.ReactElement, user: AuthUser = adminUser) {
  return render(<AuthProvider initialUser={user}>{ui}</AuthProvider>);
}

describe('StateActions', () => {
  it('shows "Start Experiment" for DRAFT state', () => {
    renderWithAuth(<StateActions state="DRAFT" onTransition={vi.fn()} />);
    expect(screen.getByText('Start Experiment')).toBeInTheDocument();
  });

  it('shows "Conclude Experiment" for RUNNING state', () => {
    renderWithAuth(<StateActions state="RUNNING" onTransition={vi.fn()} />);
    expect(screen.getByText('Conclude Experiment')).toBeInTheDocument();
  });

  it('shows "Archive Experiment" for CONCLUDED state', () => {
    renderWithAuth(<StateActions state="CONCLUDED" onTransition={vi.fn()} />);
    expect(screen.getByText('Archive Experiment')).toBeInTheDocument();
  });

  it('renders nothing for STARTING state', () => {
    const { container } = renderWithAuth(<StateActions state="STARTING" onTransition={vi.fn()} />);
    expect(container.innerHTML).toBe('');
  });

  it('renders nothing for CONCLUDING state', () => {
    const { container } = renderWithAuth(<StateActions state="CONCLUDING" onTransition={vi.fn()} />);
    expect(container.innerHTML).toBe('');
  });

  it('opens confirmation dialog on click', async () => {
    const user = userEvent.setup();
    renderWithAuth(<StateActions state="DRAFT" onTransition={vi.fn()} />);

    await user.click(screen.getByText('Start Experiment'));
    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();
    expect(screen.getByText('Start')).toBeInTheDocument();
  });

  it('calls onTransition when confirmed', async () => {
    const user = userEvent.setup();
    const onTransition = vi.fn().mockResolvedValue(undefined);
    renderWithAuth(<StateActions state="DRAFT" onTransition={onTransition} />);

    await user.click(screen.getByText('Start Experiment'));
    await user.click(screen.getByText('Start'));

    await waitFor(() => {
      expect(onTransition).toHaveBeenCalledWith('start');
    });
  });

  it('does not call onTransition when cancelled', async () => {
    const user = userEvent.setup();
    const onTransition = vi.fn().mockResolvedValue(undefined);
    renderWithAuth(<StateActions state="DRAFT" onTransition={onTransition} />);

    await user.click(screen.getByText('Start Experiment'));
    await user.click(screen.getByText('Cancel'));

    expect(onTransition).not.toHaveBeenCalled();
  });

  // --- RBAC tests ---
  it('disables Start button for viewer role', () => {
    renderWithAuth(<StateActions state="DRAFT" onTransition={vi.fn()} />, viewerUser);
    const btn = screen.getByText('Start Experiment');
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute('title', expect.stringContaining('Requires Experimenter role'));
  });

  it('disables Start button for analyst role', () => {
    renderWithAuth(<StateActions state="DRAFT" onTransition={vi.fn()} />, analystUser);
    const btn = screen.getByText('Start Experiment');
    expect(btn).toBeDisabled();
  });

  it('enables Start button for experimenter role', () => {
    renderWithAuth(<StateActions state="DRAFT" onTransition={vi.fn()} />, experimenterUser);
    const btn = screen.getByText('Start Experiment');
    expect(btn).not.toBeDisabled();
  });

  it('disables Archive button for experimenter role', () => {
    renderWithAuth(<StateActions state="CONCLUDED" onTransition={vi.fn()} />, experimenterUser);
    const btn = screen.getByText('Archive Experiment');
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute('title', expect.stringContaining('Requires Admin role'));
  });

  it('enables Archive button for admin role', () => {
    renderWithAuth(<StateActions state="CONCLUDED" onTransition={vi.fn()} />, adminUser);
    const btn = screen.getByText('Archive Experiment');
    expect(btn).not.toBeDisabled();
  });
});
