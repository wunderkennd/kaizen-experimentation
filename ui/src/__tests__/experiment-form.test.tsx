import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import NewExperimentPage from '@/app/experiments/new/page';

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

describe('New Experiment Page', () => {
  it('renders the create experiment form', () => {
    render(<NewExperimentPage />);

    expect(screen.getByRole('heading', { name: 'Create Experiment' })).toBeInTheDocument();
    expect(screen.getByLabelText(/Name/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Owner Email/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Experiment Type/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Layer ID/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Primary Metric/)).toBeInTheDocument();
  });

  it('shows breadcrumb with link to experiments list', () => {
    render(<NewExperimentPage />);

    const experimentsLink = screen.getByText('Experiments');
    expect(experimentsLink.closest('a')).toHaveAttribute('href', '/');
  });

  it('renders default two variants (control + treatment)', () => {
    render(<NewExperimentPage />);

    // Default variants: control and treatment
    const nameInputs = screen.getAllByLabelText(/Variant \d+ name/);
    expect(nameInputs).toHaveLength(2);
  });

  it('shows validation error when submitting empty form', async () => {
    const user = userEvent.setup();
    render(<NewExperimentPage />);

    const submitBtn = screen.getByRole('button', { name: 'Create Experiment' });
    await user.click(submitBtn);

    expect(screen.getByRole('alert')).toHaveTextContent('Experiment name is required');
  });

  it('shows validation error for missing owner email', async () => {
    const user = userEvent.setup();
    render(<NewExperimentPage />);

    await user.type(screen.getByLabelText(/Name/), 'test_experiment');
    await user.click(screen.getByRole('button', { name: 'Create Experiment' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Owner email is required');
  });

  it('shows validation error for missing layer ID', async () => {
    const user = userEvent.setup();
    render(<NewExperimentPage />);

    await user.type(screen.getByLabelText(/Name/), 'test_experiment');
    await user.type(screen.getByLabelText(/Owner Email/), 'test@streamco.com');
    await user.click(screen.getByRole('button', { name: 'Create Experiment' }));

    expect(screen.getByRole('alert')).toHaveTextContent('Layer ID is required');
  });

  it('successfully creates experiment and navigates to detail page', async () => {
    const user = userEvent.setup();
    render(<NewExperimentPage />);

    // Fill required fields
    await user.type(screen.getByLabelText(/Name/), 'my_new_experiment');
    await user.type(screen.getByLabelText(/Owner Email/), 'test@streamco.com');
    await user.type(screen.getByLabelText(/Layer ID/), 'layer-test');
    await user.type(screen.getByLabelText(/Primary Metric/), 'conversion_rate');

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
    render(<NewExperimentPage />);

    const typeSelect = screen.getByLabelText(/Experiment Type/);
    await user.selectOptions(typeSelect, 'INTERLEAVING');

    expect(typeSelect).toHaveValue('INTERLEAVING');
  });

  it('can add and remove variants', async () => {
    const user = userEvent.setup();
    render(<NewExperimentPage />);

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

  it('can add and remove guardrails', async () => {
    const user = userEvent.setup();
    render(<NewExperimentPage />);

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

  it('can toggle sequential testing options', async () => {
    const user = userEvent.setup();
    render(<NewExperimentPage />);

    // Sequential testing is off by default
    expect(screen.queryByLabelText(/Method/)).not.toBeInTheDocument();

    // Enable sequential testing
    await user.click(screen.getByLabelText(/Enable sequential testing/));
    expect(screen.getByLabelText(/Method/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Planned Looks/)).toBeInTheDocument();
    expect(screen.getByLabelText(/Overall Alpha/)).toBeInTheDocument();
  });

  it('shows traffic sum indicator', () => {
    render(<NewExperimentPage />);

    // Default: 50% + 50% = 100.0%
    expect(screen.getByText('Total traffic: 100.0%')).toBeInTheDocument();
  });
});
