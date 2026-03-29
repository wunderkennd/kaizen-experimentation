import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';

const SLATE_EXP_ID = 'cccccccc-cccc-cccc-cccc-cccccccccccc';
const NON_SLATE_EXP_ID = '11111111-1111-1111-1111-111111111111';

let mockExperimentId = SLATE_EXP_ID;

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: mockExperimentId }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

vi.mock('@/lib/toast-context', () => ({
  useToast: () => ({ addToast: vi.fn(), removeToast: vi.fn(), toasts: [] }),
  ToastProvider: ({ children }: { children: React.ReactNode }) => children,
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
    BarChart: Passthrough,
    Bar: Noop,
    XAxis: Noop,
    YAxis: Noop,
    CartesianGrid: Noop,
    Tooltip: Noop,
    Cell: Noop,
    Legend: Noop,
  };
});

import ExperimentDetailPage from '@/app/experiments/[id]/page';
import { SlateResultsPanel } from '@/components/slate/SlateResultsPanel';
import { SlateAssignmentForm } from '@/components/slate/SlateAssignmentForm';
import { SlatePositionBiasChart } from '@/components/slate/SlatePositionBiasChart';

const defaultUser: AuthUser = { email: 'test@streamco.com', role: 'admin' };

function renderDetail(user: AuthUser = defaultUser) {
  return render(
    <AuthProvider initialUser={user}>
      <ExperimentDetailPage />
    </AuthProvider>,
  );
}

// ── SlateResultsPanel ──────────────────────────────────────────────────────

describe('SlateResultsPanel', () => {
  const mockResponse = {
    slateItemIds: ['item-a', 'item-b', 'item-c'],
    slotProbabilities: [0.85, 0.60, 0.35],
    slateProbability: 0.1785,
  };

  it('renders ranked list of slate items', () => {
    render(<SlateResultsPanel response={mockResponse} />);
    expect(screen.getByText('item-a')).toBeInTheDocument();
    expect(screen.getByText('item-b')).toBeInTheDocument();
    expect(screen.getByText('item-c')).toBeInTheDocument();
  });

  it('shows position badges 1, 2, 3', () => {
    render(<SlateResultsPanel response={mockResponse} />);
    expect(screen.getByText('1')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('shows probability badges for each slot', () => {
    render(<SlateResultsPanel response={mockResponse} />);
    expect(screen.getByText('85.0%')).toBeInTheDocument();
    expect(screen.getByText('60.0%')).toBeInTheDocument();
    expect(screen.getByText('35.0%')).toBeInTheDocument();
  });

  it('shows overall slate probability', () => {
    render(<SlateResultsPanel response={mockResponse} />);
    expect(screen.getByText(/overall slate probability/i)).toBeInTheDocument();
  });

  it('renders testid', () => {
    render(<SlateResultsPanel response={mockResponse} />);
    expect(screen.getByTestId('slate-results-panel')).toBeInTheDocument();
  });
});

// ── SlatePositionBiasChart ─────────────────────────────────────────────────

describe('SlatePositionBiasChart', () => {
  it('shows loading state initially', async () => {
    render(<SlatePositionBiasChart experimentId={SLATE_EXP_ID} />);
    expect(screen.getByRole('status', { name: /loading position bias/i })).toBeInTheDocument();
  });

  it('renders chart after data loads', async () => {
    render(<SlatePositionBiasChart experimentId={SLATE_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByTestId('slate-position-bias-chart')).toBeInTheDocument();
    });
    expect(screen.getByText(/per-position ctr/i)).toBeInTheDocument();
  });

  it('shows estimated policy value from LIPS OPE', async () => {
    render(<SlatePositionBiasChart experimentId={SLATE_EXP_ID} />);
    await waitFor(() => {
      expect(screen.getByText(/policy value/i)).toBeInTheDocument();
    });
    expect(screen.getByText(/0\.1423/)).toBeInTheDocument();
  });

  it('shows no-data message for unknown experiment', async () => {
    render(<SlatePositionBiasChart experimentId="00000000-0000-0000-0000-000000000000" />);
    await waitFor(() => {
      expect(screen.getByText(/no lips ope data/i)).toBeInTheDocument();
    });
  });
});

// ── SlateAssignmentForm ────────────────────────────────────────────────────

describe('SlateAssignmentForm', () => {
  it('renders form fields', () => {
    render(<SlateAssignmentForm experimentId={SLATE_EXP_ID} />);
    expect(screen.getByLabelText(/user id/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/number of slots/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/candidate item ids/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /get slate assignment/i })).toBeInTheDocument();
  });

  it('renders testid', () => {
    render(<SlateAssignmentForm experimentId={SLATE_EXP_ID} />);
    expect(screen.getByTestId('slate-assignment-form')).toBeInTheDocument();
  });

  it('submits and renders SlateResultsPanel on success', async () => {
    render(<SlateAssignmentForm experimentId={SLATE_EXP_ID} />);
    fireEvent.click(screen.getByRole('button', { name: /get slate assignment/i }));

    await waitFor(() => {
      expect(screen.getByTestId('slate-results-panel')).toBeInTheDocument();
    });
  });

  it('shows error when n_slots exceeds candidates', async () => {
    render(<SlateAssignmentForm experimentId={SLATE_EXP_ID} />);
    const nSlotsInput = screen.getByLabelText(/number of slots/i);
    fireEvent.change(nSlotsInput, { target: { value: '20' } });

    const candidatesArea = screen.getByLabelText(/candidate item ids/i);
    fireEvent.change(candidatesArea, { target: { value: 'item-a, item-b' } });

    fireEvent.click(screen.getByRole('button', { name: /get slate assignment/i }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument();
    });
    expect(screen.getByRole('alert').textContent).toMatch(/n_slots.*cannot exceed/i);
  });

  it('shows error when no candidates provided', async () => {
    render(<SlateAssignmentForm experimentId={SLATE_EXP_ID} />);
    const candidatesArea = screen.getByLabelText(/candidate item ids/i);
    fireEvent.change(candidatesArea, { target: { value: '' } });
    fireEvent.click(screen.getByRole('button', { name: /get slate assignment/i }));

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeInTheDocument();
    });
    expect(screen.getByRole('alert').textContent).toMatch(/at least one candidate/i);
  });
});

// ── ExperimentDetailPage — Slate tab ──────────────────────────────────────

describe('ExperimentDetailPage — Slate tab visibility', () => {
  beforeEach(() => {
    mockExperimentId = SLATE_EXP_ID;
  });

  it('shows Slate tab section for SLATE experiment type', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByTestId('slate-tab')).toBeInTheDocument();
    });
  });

  it('shows "Slate" tab label', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getByText('Slate')).toBeInTheDocument();
    });
  });
});

describe('ExperimentDetailPage — Slate tab hidden for non-SLATE', () => {
  beforeEach(() => {
    mockExperimentId = NON_SLATE_EXP_ID;
  });

  it('does not show Slate tab for AB experiment', async () => {
    renderDetail();

    await waitFor(() => {
      expect(screen.getAllByText('homepage_recs_v2').length).toBeGreaterThan(0);
    });

    expect(screen.queryByTestId('slate-tab')).not.toBeInTheDocument();
  });
});
