import { render, screen, waitFor, within } from '@testing-library/react';
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
import type { QueryLogEntry } from '@/lib/types';

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

    it('focuses Cancel button on open', () => {
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

      expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Cancel' }));
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
          <ExperimentListPage />
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
          <ExperimentListPage />
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
          <ExperimentListPage />
        </AuthProvider>,
      );

      await waitFor(() => {
        expect(screen.getByText('homepage_recs_v2')).toBeInTheDocument();
      });

      const nameButton = screen.getByRole('button', { name: /Name/ });
      nameButton.focus();
      await user.keyboard('{Enter}');

      // After sorting by name ascending, first row should be adaptive_bitrate_v3
      await waitFor(() => {
        const rows = screen.getAllByRole('row');
        const firstDataRow = rows[1];
        expect(within(firstDataRow).getByText('adaptive_bitrate_v3')).toBeInTheDocument();
      });
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
});
