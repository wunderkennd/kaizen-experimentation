import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { QoeTab } from '@/components/qoe-tab';
import { NoveltyTab } from '@/components/novelty-tab';

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
    Bar: Noop,
    Scatter: Noop,
    Line: Noop,
    Area: Noop,
    XAxis: Noop,
    YAxis: Noop,
    CartesianGrid: Noop,
    ReferenceLine: Noop,
    Tooltip: Noop,
    Legend: Noop,
    ErrorBar: Noop,
    Cell: Noop,
  };
});

describe('QoE Dashboard - adaptive_bitrate_v3', () => {
  it('renders loading spinner initially', () => {
    render(<QoeTab experimentId="22222222-2222-2222-2222-222222222222" />);
    expect(document.querySelector('.animate-spin')).toBeInTheDocument();
  });

  it('renders overall status banner', async () => {
    render(<QoeTab experimentId="22222222-2222-2222-2222-222222222222" />);

    await waitFor(() => {
      expect(screen.getByText(/Overall QoE: Warning/)).toBeInTheDocument();
    });
  });

  it('renders all QoE metric cards', async () => {
    render(<QoeTab experimentId="22222222-2222-2222-2222-222222222222" />);

    await waitFor(() => {
      expect(screen.getByText('Time to First Frame')).toBeInTheDocument();
    });

    expect(screen.getByText('Rebuffer Ratio')).toBeInTheDocument();
    expect(screen.getByText('Average Bitrate')).toBeInTheDocument();
    expect(screen.getByText('Resolution Switches')).toBeInTheDocument();
    expect(screen.getByText('Startup Failure Rate')).toBeInTheDocument();
  });

  it('shows treatment and control values', async () => {
    render(<QoeTab experimentId="22222222-2222-2222-2222-222222222222" />);

    await waitFor(() => {
      // TTFF: treatment 950ms, control 1200ms
      expect(screen.getByText('950 ms')).toBeInTheDocument();
    });

    expect(screen.getByText('1200 ms')).toBeInTheDocument();
  });

  it('displays Good and Warning status badges', async () => {
    render(<QoeTab experimentId="22222222-2222-2222-2222-222222222222" />);

    await waitFor(() => {
      // 4 metrics are GOOD, 1 is WARNING
      const goodBadges = screen.getAllByText('Good');
      expect(goodBadges.length).toBe(4);
    });

    // startup_failure_rate card badge
    expect(screen.getAllByText('Warning').length).toBeGreaterThanOrEqual(1);
    // Overall status banner
    expect(screen.getByText(/Overall QoE: Warning/)).toBeInTheDocument();
  });

  it('shows improvement/regression indicators', async () => {
    render(<QoeTab experimentId="22222222-2222-2222-2222-222222222222" />);

    await waitFor(() => {
      // Most metrics should show improvement
      const improvements = screen.getAllByText('improvement');
      expect(improvements.length).toBeGreaterThanOrEqual(3);
    });

    // startup_failure_rate should show regression (0.020 → 0.035)
    expect(screen.getByText('regression')).toBeInTheDocument();
  });

  it('shows empty state for experiment without QoE data', async () => {
    render(<QoeTab experimentId="99999999-9999-9999-9999-999999999999" />);

    await waitFor(() => {
      expect(screen.getByText('No QoE dashboard available for this experiment.')).toBeInTheDocument();
    });
  });
});

describe('Novelty Tab - Decay Curve', () => {
  it('renders novelty detection banner', async () => {
    render(<NoveltyTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('Novelty Effect Detected')).toBeInTheDocument();
    });
  });

  it('shows key novelty metrics', async () => {
    render(<NoveltyTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('Current Effect')).toBeInTheDocument();
    });

    expect(screen.getByText('Steady-State Projection')).toBeInTheDocument();
    expect(screen.getByText('Novelty Amplitude')).toBeInTheDocument();
    expect(screen.getByText('Decay Constant')).toBeInTheDocument();
  });

  it('displays decay constant value', async () => {
    render(<NoveltyTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('4.2 days')).toBeInTheDocument();
    });
  });

  it('renders Treatment Effect Over Time chart section', async () => {
    render(<NoveltyTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('Treatment Effect Over Time')).toBeInTheDocument();
    });

    // Chart container should be present (mocked recharts)
    expect(screen.getAllByTestId('responsive-container').length).toBeGreaterThanOrEqual(1);
  });

  it('shows stability status with days remaining', async () => {
    render(<NoveltyTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('Stability Status')).toBeInTheDocument();
    });

    expect(screen.getByText(/~6 days until projected stability/)).toBeInTheDocument();
  });
});
