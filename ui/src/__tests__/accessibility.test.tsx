import { render, screen, waitFor, within, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { ConfirmDialog } from '@/components/confirm-dialog';
import { ExperimentForm } from '@/components/experiment-form';
import { StateBadge } from '@/components/state-badge';
import { NavHeader } from '@/components/nav-header';
import { QueryLogTable } from '@/components/query-log-table';
import { ToastProvider } from '@/lib/toast-context';
import ExperimentListPage from '@/app/page';
import ResultsPage from '@/app/experiments/[id]/results/page';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';
import type { QueryLogEntry, Experiment } from '@/lib/types';
import { ExperimentSelector } from '@/components/experiment-selector';
import { MonitoringHealthTable } from '@/components/monitoring-health-table';
import { ExperimentPortfolioTable } from '@/components/experiment-portfolio-table';
import { StartingChecklist } from '@/components/starting-checklist';
import type { PortfolioExperiment } from '@/lib/types';

const defaultUser: AuthUser = { email: 'test@streamco.com', role: 'experimenter' };

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: '11111111-1111-1111-1111-111111111111' }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/',
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

// Mock next/dynamic to eagerly resolve dynamic imports in tests
vi.mock('next/dynamic', () => ({
  default: (loader: () => Promise<{ default: React.ComponentType<unknown> }>) => {
    let Comp: React.ComponentType<unknown> | null = null;
    loader().then((mod) => { Comp = mod.default; });
    return function DynamicMock(props: Record<string, unknown>) {
      return Comp ? <Comp {...props} /> : null;
    };
  },
}));

// Mock recharts
vi.mock('recharts', async () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div data-testid="responsive-container">{children}</div>
  );
  const Noop = () => null;

  return {
    ResponsiveContainer: Passthrough,
    ComposedChart: Passthrough,
    BarChart: Passthrough,
    Bar: Noop,
    Line: Noop,
    Scatter: Noop,
    XAxis: Noop,
    YAxis: Noop,
    CartesianGrid: Noop,
    ReferenceLine: Noop,
    Tooltip: Noop,
    ErrorBar: Noop,
    Cell: Noop,
    Legend: Noop,
    Area: Noop,
  };
});

describe('Accessibility', () => {
  // --- ConfirmDialog ---

  describe('ConfirmDialog', () => {
    it('has role="alertdialog" and aria-modal when open', () => {
      render(
        <ConfirmDialog
          open={true}
          title="Confirm Action"
          message="Are you sure?"
          confirmLabel="Yes"
          onConfirm={() => {}}
          onCancel={() => {}}
        />,
      );

      const dialog = screen.getByRole('alertdialog');
      expect(dialog).toHaveAttribute('aria-modal', 'true');
      expect(dialog).toHaveAttribute('aria-labelledby');
      expect(dialog).toHaveAttribute('aria-describedby');
    });

    it('Escape key calls onCancel', async () => {
      const onCancel = vi.fn();
      const user = userEvent.setup();

      render(
        <ConfirmDialog
          open={true}
          title="Confirm"
          message="Test"
          confirmLabel="OK"
          onConfirm={() => {}}
          onCancel={onCancel}
        />,
      );

      await user.keyboard('{Escape}');
      expect(onCancel).toHaveBeenCalledTimes(1);
    });

    it('focuses Cancel button on open and button has accessibility focus ring classes', () => {
      render(
        <ConfirmDialog
          open={true}
          title="Confirm"
          message="Test"
          confirmLabel="OK"
          onConfirm={() => {}}
          onCancel={() => {}}
        />,
      );

      const cancelButton = screen.getByRole('button', { name: 'Cancel' });
      expect(document.activeElement).toBe(cancelButton);
      expect(cancelButton).toHaveClass('focus-visible:ring-2');
      expect(cancelButton).toHaveClass('focus-visible:ring-indigo-500');
      expect(cancelButton).toHaveClass('focus-visible:ring-offset-2');
    });

    it('title and message are linked via aria-labelledby/describedby', () => {
      render(
        <ConfirmDialog
          open={true}
          title="Delete Experiment"
          message="This cannot be undone."
          confirmLabel="Delete"
          onConfirm={() => {}}
          onCancel={() => {}}
        />,
      );

      const dialog = screen.getByRole('alertdialog');
      const titleId = dialog.getAttribute('aria-labelledby')!;
      const descId = dialog.getAttribute('aria-describedby')!;
      expect(document.getElementById(titleId)?.textContent).toBe('Delete Experiment');
      expect(document.getElementById(descId)?.textContent).toBe('This cannot be undone.');
    });

    it('shows a loading spinner when loading is true', () => {
      render(
        <ConfirmDialog
          open={true}
          title="Loading Test"
          message="Testing spinner"
          confirmLabel="OK"
          onConfirm={() => {}}
          onCancel={() => {}}
          loading={true}
        />,
      );

      const spinner = screen.getByTestId('confirm-spinner');
      expect(spinner).toBeInTheDocument();
      expect(spinner).toHaveAttribute('aria-hidden', 'true');
      expect(screen.getByText('Processing...')).toBeInTheDocument();
    });
  });

  // --- ExperimentForm ---

  describe('ExperimentForm', () => {
    it('all required inputs on step 1 have aria-required="true"', () => {
      render(
        <AuthProvider initialUser={defaultUser}>
          <ExperimentForm onSubmit={async () => {}} />
        </AuthProvider>,
      );

      // Wizard step 1 (Basics) is shown by default — Primary Metric is on step 4
      expect(screen.getByLabelText(/^Name/)).toHaveAttribute('aria-required', 'true');
      expect(screen.getByLabelText(/Owner Email/)).toHaveAttribute('aria-required', 'true');
      expect(screen.getByLabelText(/Experiment Type/)).toHaveAttribute('aria-required', 'true');
      expect(screen.getByLabelText(/Layer ID/)).toHaveAttribute('aria-required', 'true');
    });
  });

  // --- Loading spinner ---

  describe('Loading spinners', () => {
    it('experiment list loading spinner has role="status" and sr-only text', () => {
      render(
        <AuthProvider initialUser={defaultUser}>
          <ToastProvider>
            <ExperimentListPage />
          </ToastProvider>
        </AuthProvider>,
      );

      const spinner = screen.getByRole('status');
      expect(spinner).toHaveAttribute('aria-label', 'Loading');
      expect(within(spinner).getByText('Loading')).toHaveClass('sr-only');
    });
  });

  // --- SortableHeader ---

  describe('SortableHeader', () => {
    it('renders a button inside <th> for keyboard access', async () => {
      render(
        <AuthProvider initialUser={defaultUser}>
          <ToastProvider>
            <ExperimentListPage />
          </ToastProvider>
        </AuthProvider>,
      );

      await waitFor(() => {
        expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
      });

      const nameButton = screen.getByRole('button', { name: /Name/ });
      expect(nameButton.tagName).toBe('BUTTON');
      expect(nameButton.closest('th')).toBeInTheDocument();
    });

    it('keyboard Enter triggers sort on button', async () => {
      const user = userEvent.setup();
      render(
        <AuthProvider initialUser={defaultUser}>
          <ToastProvider>
            <ExperimentListPage />
          </ToastProvider>
        </AuthProvider>,
      );

      await waitFor(() => {
        expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
      });

      const nameButton = screen.getByRole('button', { name: /Name/ });
      nameButton.focus();
      await user.keyboard('{Enter}');

      // Verify the sort is ascending by checking aria-sort attribute
      await waitFor(() => {
        const nameHeader = screen.getByRole('button', { name: /Name/ }).closest('th');
        expect(nameHeader?.getAttribute('aria-sort')).toBe('ascending');
      });
      // Verify alphabetically-first experiment is visible in the list
      expect(screen.getByText('adaptive_bitrate_v3')).toBeInTheDocument();
    });
  });

  // --- ExperimentRow ---

  describe('ExperimentRow', () => {
    it('links have focus-visible styling for keyboard accessibility', async () => {
      render(
        <AuthProvider initialUser={defaultUser}>
          <ToastProvider>
            <ExperimentListPage />
          </ToastProvider>
        </AuthProvider>,
      );

      await waitFor(() => {
        expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
      });

      const nameLink = screen.getByRole('link', { name: 'homepage_recs_v2' });
      expect(nameLink).toHaveClass('focus-visible:ring-2');
      expect(nameLink).toHaveClass('focus-visible:ring-indigo-500');
    });
  });

  describe('MonitoringHealthTable', () => {
    it('experiment name links have focus-visible styling for keyboard accessibility', () => {
      const mockRunningExperiments: Experiment[] = [
        {
          experimentId: 'exp-running-1',
          name: 'Running Exp 1',
          description: 'First mock experiment',
          ownerEmail: 'owner1@streamco.com',
          type: 'AB',
          state: 'RUNNING',
          variants: [],
          layerId: 'layer-1',
          hashSalt: 'salt',
          primaryMetricId: 'metric-1',
          secondaryMetricIds: [],
          guardrailConfigs: [],
          guardrailAction: 'ALERT_ONLY',
          isCumulativeHoldout: false,
          createdAt: '2026-01-01T00:00:00Z',
          startedAt: '2026-01-01T00:00:00Z',
        },
      ];

      render(
        <MonitoringHealthTable
          experiments={mockRunningExperiments}
          analysisResults={{}}
          guardrailStatuses={{}}
        />,
      );

      const nameLink = screen.getByRole('link', { name: 'Running Exp 1' });
      expect(nameLink).toHaveClass('focus-visible:ring-2');
      expect(nameLink).toHaveClass('focus-visible:ring-indigo-500');
      expect(nameLink).toHaveClass('focus-visible:ring-offset-2');
    });
  });

  describe('ExperimentPortfolioTable', () => {
    it('experiment name links have focus-visible styling for keyboard accessibility', () => {
      const mockPortfolioExperiments: PortfolioExperiment[] = [
        {
          experimentId: 'exp-portfolio-1',
          name: 'Portfolio Exp 1',
          effectSize: 0.1,
          variance: 0.01,
          allocatedTrafficPct: 0.5,
          priorityScore: 0.8,
          userSegments: ['all'],
        },
      ];

      render(<ExperimentPortfolioTable experiments={mockPortfolioExperiments} />);

      const nameLink = screen.getByRole('link', { name: 'Portfolio Exp 1' });
      expect(nameLink).toHaveClass('focus-visible:ring-2');
      expect(nameLink).toHaveClass('focus-visible:ring-indigo-500');
      expect(nameLink).toHaveClass('focus-visible:ring-offset-2');
    });
  });

  // --- Charts ---

  describe('Charts', () => {
    it('forest plot has role="img" with aria-label', async () => {
      render(<ResultsPage />);

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
      });

      const forestPlot = screen.getByRole('img', { name: /Forest plot showing treatment effects/ });
      expect(forestPlot).toBeInTheDocument();
    });
  });

  // --- Tab panels ---

  describe('Tab panels', () => {
    it('has role="tablist" on nav and role="tabpanel" on content', async () => {
      render(<ResultsPage />);

      await waitFor(() => {
        expect(screen.getByRole('heading', { name: 'Results Dashboard' })).toBeInTheDocument();
      });

      expect(screen.getByRole('tablist')).toBeInTheDocument();
      expect(screen.getByRole('tabpanel')).toBeInTheDocument();

      // Check tab has aria-controls
      const overviewTab = screen.getByRole('tab', { name: 'Overview' });
      expect(overviewTab).toHaveAttribute('aria-controls', 'tabpanel-overview');
      expect(overviewTab).toHaveAttribute('id', 'tab-overview');
    });
  });

  // --- QueryLogTable ---

  describe('QueryLogTable', () => {
    const entries: QueryLogEntry[] = [
      {
        experimentId: 'exp-1',
        metricId: 'click_through_rate',
        sqlText: 'SELECT count(*) FROM events WHERE event_type = \'click\'',
        rowCount: 1000,
        durationMs: 250,
        computedAt: new Date().toISOString(),
      },
    ];

    it('SQL preview button has aria-expanded that toggles on click', async () => {
      const user = userEvent.setup();
      render(
        <ToastProvider>
          <QueryLogTable entries={entries} onExport={() => {}} exporting={false} />
        </ToastProvider>,
      );

      const toggleButton = screen.getByRole('button', { name: /Toggle SQL preview/ });
      expect(toggleButton).toHaveAttribute('aria-expanded', 'false');

      await user.click(toggleButton);
      expect(toggleButton).toHaveAttribute('aria-expanded', 'true');

      await user.click(toggleButton);
      expect(toggleButton).toHaveAttribute('aria-expanded', 'false');
    });
  });

  // --- NavHeader ---

  describe('NavHeader', () => {
    it('has navigation landmark', () => {
      render(
        <AuthProvider initialUser={defaultUser}>
          <NavHeader />
        </AuthProvider>,
      );

      expect(screen.getByRole('navigation', { name: 'Main navigation' })).toBeInTheDocument();
    });
  });

  // --- StateBadge ---

  describe('StateBadge', () => {
    it('decorative dot has aria-hidden="true"', () => {
      const { container } = render(<StateBadge state="RUNNING" />);

      // The dot is the first child span inside the badge
      const dot = container.querySelector('span > span');
      expect(dot).toHaveAttribute('aria-hidden', 'true');
    });
  });

  // --- ExperimentSelector ---

  describe('ExperimentSelector Accessibility', () => {
    const mockExperiments: Experiment[] = [
      {
        experimentId: 'exp-1',
        name: 'Running Exp 1',
        description: 'First mock experiment',
        ownerEmail: 'owner1@streamco.com',
        type: 'AB',
        state: 'RUNNING',
        variants: [
          { variantId: 'v1', name: 'Control', trafficFraction: 0.5, isControl: true, payloadJson: '{}' },
          { variantId: 'v2', name: 'Treatment', trafficFraction: 0.5, isControl: false, payloadJson: '{}' },
        ],
        layerId: 'layer-1',
        hashSalt: 'salt',
        primaryMetricId: 'metric-1',
        secondaryMetricIds: [],
        guardrailConfigs: [],
        guardrailAction: 'ALERT_ONLY',
        isCumulativeHoldout: false,
        createdAt: '2026-01-01T00:00:00Z',
      },
      {
        experimentId: 'exp-2',
        name: 'Concluded Exp 2',
        description: 'Second mock experiment',
        ownerEmail: 'owner2@streamco.com',
        type: 'AB',
        state: 'CONCLUDED',
        variants: [
          { variantId: 'v3', name: 'Control', trafficFraction: 0.5, isControl: true, payloadJson: '{}' },
          { variantId: 'v4', name: 'Treatment', trafficFraction: 0.5, isControl: false, payloadJson: '{}' },
        ],
        layerId: 'layer-2',
        hashSalt: 'salt2',
        primaryMetricId: 'metric-1',
        secondaryMetricIds: [],
        guardrailConfigs: [],
        guardrailAction: 'ALERT_ONLY',
        isCumulativeHoldout: false,
        createdAt: '2026-01-01T00:00:00Z',
      },
    ];

    it('correctly associates label with the input element via htmlFor and id', () => {
      render(
        <ExperimentSelector
          experiments={mockExperiments}
          selectedIds={[]}
          onSelect={vi.fn()}
          onRemove={vi.fn()}
        />,
      );

      const label = screen.getByText(/Select experiments to compare/);
      expect(label).toHaveAttribute('for', 'experiment-search');

      const input = screen.getByRole('textbox', { name: 'Search experiments' });
      expect(input).toHaveAttribute('id', 'experiment-search');
    });

    it('input has focus-visible styling classes', () => {
      render(
        <ExperimentSelector
          experiments={mockExperiments}
          selectedIds={[]}
          onSelect={vi.fn()}
          onRemove={vi.fn()}
        />,
      );

      const input = screen.getByRole('textbox', { name: 'Search experiments' });
      expect(input).toHaveClass('focus-visible:ring-2');
      expect(input).toHaveClass('focus-visible:ring-indigo-500');
      expect(input).toHaveClass('focus-visible:ring-offset-2');
    });

    it('provides keyboard-navigable and keyboard-selectable dropdown options', async () => {
      const user = userEvent.setup();
      const onSelect = vi.fn();

      render(
        <ExperimentSelector
          experiments={mockExperiments}
          selectedIds={[]}
          onSelect={onSelect}
          onRemove={vi.fn()}
        />,
      );

      const input = screen.getByRole('textbox', { name: 'Search experiments' });
      await user.click(input);
      await user.keyboard('Running');

      const options = await screen.findAllByRole('option');
      expect(options).toHaveLength(1);
      const option = options[0];

      // Verify option is in the tab order (tabIndex={0}) and has focus styles
      expect(option).toHaveAttribute('tabIndex', '0');
      expect(option).toHaveClass('focus-visible:ring-2');
      expect(option).toHaveClass('focus-visible:ring-indigo-500');

      // Trigger selection with Enter key
      await user.click(input); // focus input again
      fireEvent.keyDown(option, { key: 'Enter', code: 'Enter' });
      expect(onSelect).toHaveBeenCalledWith('exp-1');

      // Trigger selection with Space key
      onSelect.mockClear();
      fireEvent.keyDown(option, { key: ' ', code: 'Space' });
      expect(onSelect).toHaveBeenCalledWith('exp-1');
    });

    it('close button on selected chip has accessibility focus ring classes', () => {
      render(
        <ExperimentSelector
          experiments={mockExperiments}
          selectedIds={['exp-1']}
          onSelect={vi.fn()}
          onRemove={vi.fn()}
        />,
      );

      const removeButton = screen.getByRole('button', { name: 'Remove Running Exp 1' });
      expect(removeButton).toHaveClass('focus-visible:ring-2');
      expect(removeButton).toHaveClass('focus-visible:ring-indigo-500');
      expect(removeButton).toHaveClass('focus-visible:ring-offset-1');
    });

    it('clear all button on selected chips has accessibility focus ring classes', () => {
      render(
        <ExperimentSelector
          experiments={mockExperiments}
          selectedIds={['exp-1']}
          onSelect={vi.fn()}
          onRemove={vi.fn()}
          onClearAll={vi.fn()}
        />,
      );

      const clearAllButton = screen.getByRole('button', { name: 'Clear all selections' });
      expect(clearAllButton).toHaveClass('focus-visible:ring-2');
      expect(clearAllButton).toHaveClass('focus-visible:ring-indigo-500');
      expect(clearAllButton).toHaveClass('focus-visible:ring-offset-1');
    });

    it('focuses search input on "/" keypress and handles disabled state at selection limit', async () => {
      const user = userEvent.setup();
      const { rerender } = render(
        <ExperimentSelector
          experiments={mockExperiments}
          selectedIds={[]}
          onSelect={vi.fn()}
          onRemove={vi.fn()}
        />,
      );

      const input = screen.getByRole('textbox', { name: 'Search experiments' });
      expect(input).not.toHaveFocus();

      // Press '/' to focus
      await user.keyboard('/');
      expect(input).toHaveFocus();

      // Rerender with max selections (limit of 4 reached)
      rerender(
        <ExperimentSelector
          experiments={mockExperiments}
          selectedIds={['exp-1', 'exp-2', 'exp-3', 'exp-4']}
          onSelect={vi.fn()}
          onRemove={vi.fn()}
          maxSelections={4}
        />,
      );

      // Input should be disabled, and hint "/" badge is not rendered
      const disabledInput = screen.getByRole('textbox', { name: 'Search experiments' });
      expect(disabledInput).toBeDisabled();
      expect(screen.queryByText('/')).not.toBeInTheDocument();
    });
  });

  describe('StartingChecklist', () => {
    it('has region role and includes screen reader status text for each item', () => {
      render(<StartingChecklist />);

      const region = screen.getByRole('region', { name: 'Experiment startup progress' });
      expect(region).toBeInTheDocument();

      const srTexts = screen.getAllByText(/\((Completed|In progress|Pending)\)/);
      expect(srTexts.length).toBeGreaterThan(0);
      expect(screen.getByText('(In progress)')).toBeInTheDocument();
    });
  });
});
