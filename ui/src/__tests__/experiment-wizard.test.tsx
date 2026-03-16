import { render, screen, waitFor, within } from '@testing-library/react';
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
  useRouter: () => ({ push: mockPush }),
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

/** Helper to fill basics step and advance */
async function fillBasicsAndAdvance(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText(/Name \*/), 'my_experiment');
  await user.type(screen.getByLabelText(/Owner Email/), 'test@streamco.com');
  await user.type(screen.getByLabelText(/Layer ID/), 'layer-test');
  await user.click(screen.getByRole('button', { name: 'Next' }));
}

/** Helper to fill through to review step for AB tests */
async function fillToReview(user: ReturnType<typeof userEvent.setup>) {
  // Step 1: Basics
  await fillBasicsAndAdvance(user);
  // Step 2: Type Config (AB = no extra config)
  await user.click(screen.getByRole('button', { name: 'Next' }));
  // Step 3: Variants (defaults are valid)
  await user.click(screen.getByRole('button', { name: 'Next' }));
  // Step 4: Metrics
  await user.type(screen.getByLabelText(/Primary Metric/), 'click_through_rate');
  await user.click(screen.getByRole('button', { name: 'Next' }));
}

describe('Experiment Creation Wizard', () => {
  it('renders step indicator with 5 steps', () => {
    renderNewPage();
    const nav = screen.getByLabelText('Wizard progress');
    expect(nav).toBeInTheDocument();
    expect(within(nav).getByText('Basics')).toBeInTheDocument();
    expect(within(nav).getByText('Review')).toBeInTheDocument();
  });

  it('shows Step 1 (Basics) by default', () => {
    renderNewPage();
    expect(screen.getByLabelText(/Name \*/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Owner Email/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Experiment Type/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Layer ID/)).toBeInTheDocument();
  });

  it('validates step 1 before allowing Next', async () => {
    const user = userEvent.setup();
    renderNewPage();

    // Try to advance without filling fields
    await user.click(screen.getByRole('button', { name: 'Next' }));
    expect(screen.getByRole('alert')).toHaveTextContent(/name/i);
  });

  it('navigates forward and backward with Next/Back', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await fillBasicsAndAdvance(user);

    // Should be on Step 2 (Type Config)
    expect(screen.getByText(/Configuration/)).toBeInTheDocument();

    // Go back
    await user.click(screen.getByRole('button', { name: 'Back' }));
    expect(screen.getByLabelText(/Name \*/)).toBeInTheDocument();
  });

  it('type selection changes step 2 content - INTERLEAVING shows method field', async () => {
    const user = userEvent.setup();
    renderNewPage();

    // Change type to INTERLEAVING
    await user.selectOptions(screen.getByLabelText(/Experiment Type/), 'INTERLEAVING');
    await fillBasicsAndAdvance(user);

    // Step 2 should show interleaving-specific fields
    expect(screen.getByLabelText(/Interleaving Method/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Credit Metric Event/)).toBeInTheDocument();
  });

  it('AB type shows "no extra config" on step 2', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await fillBasicsAndAdvance(user);

    // Step 2 should show "no additional configuration" for AB
    expect(screen.getByText(/No additional configuration/)).toBeInTheDocument();
  });

  it('full happy path: fill all steps, review shows values, submit calls API', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await fillToReview(user);

    // Should be on Review step
    expect(screen.getByText('Review Experiment')).toBeInTheDocument();
    expect(screen.getByText('my_experiment')).toBeInTheDocument();
    expect(screen.getByText('click_through_rate')).toBeInTheDocument();

    // Submit
    await user.click(screen.getByRole('button', { name: 'Create Experiment' }));

    await waitFor(() => {
      expect(mockPush).toHaveBeenCalledWith(
        expect.stringMatching(/^\/experiments\/[0-9a-f-]+$/),
      );
    });
  });

  it('review Edit links jump to correct step', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await fillToReview(user);

    // Click "Edit" on the Basic Information section
    const editButtons = screen.getAllByRole('button', { name: 'Edit' });
    await user.click(editButtons[0]); // First Edit = Basics

    // Should be back on step 1
    expect(screen.getByLabelText(/Name \*/)).toBeInTheDocument();
  });

  it('submit button only on review step', async () => {
    const user = userEvent.setup();
    renderNewPage();

    // Step 1: should have Next, not Create Experiment
    expect(screen.getByRole('button', { name: 'Next' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create Experiment' })).not.toBeInTheDocument();

    // Navigate to review
    await fillToReview(user);

    // Review step: should have Create Experiment, not Next
    expect(screen.getByRole('button', { name: 'Create Experiment' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Next' })).not.toBeInTheDocument();
  });

  it('viewer cannot access form (permission guard)', () => {
    const viewer: AuthUser = { email: 'viewer@streamco.com', role: 'viewer' };
    renderNewPage(viewer);

    expect(screen.getByTestId('insufficient-permissions')).toBeInTheDocument();
    expect(screen.queryByLabelText(/Name \*/)).not.toBeInTheDocument();
  });

  it('PLAYBACK_QOE shows QoE metric checkboxes on step 2', async () => {
    const user = userEvent.setup();
    renderNewPage();

    await user.selectOptions(screen.getByLabelText(/Experiment Type/), 'PLAYBACK_QOE');
    await fillBasicsAndAdvance(user);

    expect(screen.getByText('Rebuffer Ratio')).toBeInTheDocument();
    expect(screen.getByText('Time to First Frame (ms)')).toBeInTheDocument();
  });
});
