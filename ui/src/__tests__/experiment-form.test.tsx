import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import NewExperimentPage from '@/app/experiments/new/page';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';

const experimenterUser: AuthUser = { email: 'test@streamco.com', role: 'experimenter' };

function renderNewPage(user: AuthUser = experimenterUser) {
  return render(
    <AuthProvider initialUser={user}>
      <NewExperimentPage />
    </AuthProvider>,
  );
}

const mockPush = vi.fn();

vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: mockPush, replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

vi.mock('@/lib/toast-context', () => ({
  useToast: () => ({ addToast: vi.fn(), removeToast: vi.fn(), toasts: [] }),
  ToastProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

/** Navigate to a specific step by filling and clicking Next. */
async function navigateToStep(user: ReturnType<typeof userEvent.setup>, targetStep: number) {
  if (targetStep >= 1) {
    // Fill basics
    await user.type(screen.getByLabelText(/Name \*/), 'test_experiment');
    await user.type(screen.getByLabelText(/Owner Email/), 'test@streamco.com');
    await user.type(screen.getByLabelText(/Layer ID/), 'layer-test');
    await user.click(screen.getByRole('button', { name: 'Next' }));
  }
  if (targetStep >= 2) {
    // Skip type config (AB = no extra config)
    await user.click(screen.getByRole('button', { name: 'Next' }));
  }
  if (targetStep >= 3) {
    // Skip variants (defaults valid)
    await user.click(screen.getByRole('button', { name: 'Next' }));
  }
  if (targetStep >= 4) {
    // Fill metrics
    await user.type(screen.getByLabelText(/Primary Metric/), 'conversion_rate');
    await user.click(screen.getByRole('button', { name: 'Next' }));
  }
}

describe('New Experiment Page', () => {
  it('renders the create experiment form', () => {
    renderNewPage();

    expect(screen.getByRole('heading', { name: 'Create Experiment' })).toBeInTheDocument();
    expect(screen.getByLabelText(/Name \*/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Owner Email/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Experiment Type/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Layer ID/)).toBeInTheDocument();
  });

  it('shows breadcrumb with link to experiments list', () => {
    renderNewPage();

    const experimentsLink = screen.getByText('Experiments');
    expect(experimentsLink.closest('a')).toHaveAttribute('href', '/');
  });

  it('renders default two variants on variants step', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await navigateToStep(user, 2);

    // Default variants: control and treatment
    const nameInputs = screen.getAllByLabelText(/Variant \d+ name/);
    expect(nameInputs).toHaveLength(2);
  });

  it('shows validation error when submitting empty form', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await user.click(screen.getByRole('button', { name: 'Next' }));

    expect(screen.getByRole('alert')).toHaveTextContent(/name/i);
  });

  it('shows validation error for missing owner email', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await user.type(screen.getByLabelText(/Name \*/), 'test_experiment');
    await user.click(screen.getByRole('button', { name: 'Next' }));

    expect(screen.getByRole('alert')).toHaveTextContent(/email/i);
  });

  it('shows validation error for missing layer ID', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await user.type(screen.getByLabelText(/Name \*/), 'test_experiment');
    await user.type(screen.getByLabelText(/Owner Email/), 'test@streamco.com');
    await user.click(screen.getByRole('button', { name: 'Next' }));

    expect(screen.getByRole('alert')).toHaveTextContent(/layer/i);
  });

  it('successfully creates experiment and navigates to detail page', async () => {
    const user = userEvent.setup();
    renderNewPage();

    // Navigate through all steps to review
    await navigateToStep(user, 4);

    // Submit
    await user.click(screen.getByRole('button', { name: 'Create Experiment' }));

    await waitFor(() => {
      expect(mockPush).toHaveBeenCalledWith(
        expect.stringMatching(/^\/experiments\/[0-9a-f-]+$/),
      );
    });
  });

  it('can change experiment type', async () => {
    const user = userEvent.setup();
    renderNewPage();

    const typeSelect = screen.getByLabelText(/Experiment Type/);
    await user.selectOptions(typeSelect, 'INTERLEAVING');

    expect(typeSelect).toHaveValue('INTERLEAVING');
  });

  it('can add and remove variants on variants step', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await navigateToStep(user, 2);

    // Start with 2 variants
    expect(screen.getAllByLabelText(/Variant \d+ name/)).toHaveLength(2);

    // Add a variant
    await user.click(screen.getByRole('button', { name: 'Add Variant' }));
    expect(screen.getAllByLabelText(/Variant \d+ name/)).toHaveLength(3);

    // Remove the 3rd variant
    const removeButtons = screen.getAllByText('Remove');
    await user.click(removeButtons[2]);
    expect(screen.getAllByLabelText(/Variant \d+ name/)).toHaveLength(2);
  });

  it('can add and remove guardrails on metrics step', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await navigateToStep(user, 3);

    // Initially no guardrails
    expect(screen.getByText(/No guardrails configured/)).toBeInTheDocument();

    // Add guardrail
    await user.click(screen.getByRole('button', { name: 'Add Guardrail' }));
    expect(screen.getByLabelText(/Guardrail 1 metric/)).toBeInTheDocument();
    expect(screen.queryByText(/No guardrails configured/)).not.toBeInTheDocument();

    // Remove guardrail — the guardrail Remove is the last one (variants have Remove too)
    const removeButtons = screen.getAllByText('Remove');
    const guardrailRemove = removeButtons[removeButtons.length - 1];
    await user.click(guardrailRemove);
    expect(screen.getByText(/No guardrails configured/)).toBeInTheDocument();
  });

  it('can toggle sequential testing options on metrics step', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await navigateToStep(user, 3);

    // Sequential testing is off by default
    expect(screen.queryByLabelText(/Method/)).not.toBeInTheDocument();

    // Enable sequential testing
    await user.click(screen.getByLabelText(/Enable sequential testing/));
    expect(screen.getByLabelText(/Method/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Planned Looks/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Overall Alpha/)).toBeInTheDocument();
  });

  it('shows traffic sum indicator on variants step', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await navigateToStep(user, 2);

    // Default: 50% + 50% = 100.0%
    expect(screen.getByText('Total traffic: 100.0%')).toBeInTheDocument();
  });
});
