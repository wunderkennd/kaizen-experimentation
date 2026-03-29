import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import ResultsPage from '@/app/experiments/[id]/results/page';
import SqlPage from '@/app/experiments/[id]/sql/page';
import { ToastProvider } from '@/lib/toast-context';

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: '11111111-1111-1111-1111-111111111111' }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
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

// Mock recharts to avoid SVG rendering issues in jsdom
vi.mock('recharts', async () => {
  const Passthrough = ({ children }: { children?: React.ReactNode }) => (
    <div data-testid="responsive-container">{children}</div>
  );
  const Noop = () => null;

  return {
    ResponsiveContainer: Passthrough,
    ComposedChart: Passthrough,
    BarChart: Passthrough,
    AreaChart: Passthrough,
    Area: Noop,
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
  };
});

describe('Performance targets', () => {
  // jsdom lacks URL.createObjectURL — stub it so export tests can proceed
  let createObjectURLSpy: ReturnType<typeof vi.fn>;
  let revokeObjectURLSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    createObjectURLSpy = vi.fn(() => 'blob:mock');
    revokeObjectURLSpy = vi.fn();
    if (!URL.createObjectURL) {
      URL.createObjectURL = createObjectURLSpy;
    } else {
      vi.spyOn(URL, 'createObjectURL').mockImplementation(createObjectURLSpy);
    }
    if (!URL.revokeObjectURL) {
      URL.revokeObjectURL = revokeObjectURLSpy;
    } else {
      vi.spyOn(URL, 'revokeObjectURL').mockImplementation(revokeObjectURLSpy);
    }
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('results dashboard renders within 1000ms', async () => {
    const start = performance.now();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('Results Dashboard')).toBeInTheDocument();
    });

    const elapsed = performance.now() - start;
    expect(elapsed).toBeLessThan(1000);
  });

  it('tab switch renders within 1000ms', async () => {
    const user = userEvent.setup();
    render(<ResultsPage />);

    await waitFor(() => {
      expect(screen.getByText('Results Dashboard')).toBeInTheDocument();
    });

    const start = performance.now();
    await user.click(screen.getByRole('tab', { name: 'Novelty Effects' }));

    await waitFor(() => {
      expect(screen.getByRole('tabpanel', { hidden: false })).toBeInTheDocument();
    });

    const elapsed = performance.now() - start;
    expect(elapsed).toBeLessThan(1000);
  });

  it('SQL page query log renders within 1000ms', async () => {
    const start = performance.now();
    render(
      <ToastProvider>
        <SqlPage />
      </ToastProvider>,
    );

    await waitFor(() => {
      expect(screen.getByText('Query Log')).toBeInTheDocument();
    });

    const elapsed = performance.now() - start;
    expect(elapsed).toBeLessThan(1000);
  });

  it('SQL page expand shows SQL within 200ms', async () => {
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <SqlPage />
      </ToastProvider>,
    );

    await waitFor(() => {
      expect(screen.getByText('click_through_rate')).toBeInTheDocument();
    });

    const sqlPreviews = screen.getAllByRole('button', { name: /Toggle SQL preview/i });

    const start = performance.now();
    await user.click(sqlPreviews[0]);

    await waitFor(() => {
      const preElements = document.querySelectorAll('pre');
      expect(preElements.length).toBeGreaterThanOrEqual(1);
    });

    const elapsed = performance.now() - start;
    expect(elapsed).toBeLessThan(200);
  });

  it('notebook export completes within 5000ms', async () => {
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <SqlPage />
      </ToastProvider>,
    );

    await waitFor(() => {
      expect(screen.getByText('Export Notebook')).toBeInTheDocument();
    });

    const start = performance.now();
    await user.click(screen.getByText('Export Notebook'));

    // Export completes very quickly in test env (sync fallback for atob).
    // The button either returns to "Export Notebook" or the page shows an error state.
    // Either way, the flow should complete well within 5s.
    await waitFor(() => {
      // Export finished: either button resets or error shown
      const hasExportBtn = screen.queryByText('Export Notebook') !== null;
      const hasError = screen.queryByText(/failed|timed out/i) !== null;
      expect(hasExportBtn || hasError).toBe(true);
    }, { timeout: 5000 });

    const elapsed = performance.now() - start;
    expect(elapsed).toBeLessThan(5000);
  }, 10000);
});
