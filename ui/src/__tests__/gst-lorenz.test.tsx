import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { GstTrajectoryChart } from '@/components/charts/gst-trajectory-chart';
import { InterferenceTab } from '@/components/interference-tab';

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
    Line: Noop,
    Scatter: Noop,
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

describe('GST Stopping Boundary Chart', () => {
  it('renders boundary chart for homepage_recs_v2 click_through_rate', async () => {
    render(
      <GstTrajectoryChart
        experimentId="11111111-1111-1111-1111-111111111111"
        metricId="click_through_rate"
      />
    );

    await waitFor(() => {
      expect(screen.getByText(/Stopping Boundary — click_through_rate/)).toBeInTheDocument();
    });
  });

  it('shows method label badge', async () => {
    render(
      <GstTrajectoryChart
        experimentId="11111111-1111-1111-1111-111111111111"
        metricId="click_through_rate"
      />
    );

    await waitFor(() => {
      expect(screen.getByText('mSPRT')).toBeInTheDocument();
    });
  });

  it('shows planned looks and overall alpha', async () => {
    render(
      <GstTrajectoryChart
        experimentId="11111111-1111-1111-1111-111111111111"
        metricId="click_through_rate"
      />
    );

    await waitFor(() => {
      expect(screen.getByText('Planned looks: 5')).toBeInTheDocument();
    });

    expect(screen.getByText('Overall α: 0.05')).toBeInTheDocument();
  });

  it('renders chart container', async () => {
    render(
      <GstTrajectoryChart
        experimentId="11111111-1111-1111-1111-111111111111"
        metricId="click_through_rate"
      />
    );

    await waitFor(() => {
      expect(screen.getAllByTestId('responsive-container').length).toBeGreaterThanOrEqual(1);
    });
  });

  it('renders nothing for unknown experiment', async () => {
    const { container } = render(
      <GstTrajectoryChart
        experimentId="99999999-9999-9999-9999-999999999999"
        metricId="click_through_rate"
      />
    );

    await waitFor(() => {
      // Should render nothing (null return) after loading
      expect(container.querySelector('.animate-spin')).not.toBeInTheDocument();
    });

    expect(screen.queryByText(/Stopping Boundary/)).not.toBeInTheDocument();
  });
});

describe('Interference Tab - Lorenz Curve', () => {
  it('renders Lorenz curve chart section', async () => {
    render(<InterferenceTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText(/Consumption Concentration/)).toBeInTheDocument();
    });
  });

  it('shows Lorenz curve description', async () => {
    render(<InterferenceTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText(/How evenly content consumption is distributed/)).toBeInTheDocument();
    });
  });

  it('still renders interference detection banner', async () => {
    render(<InterferenceTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('Content Interference Detected')).toBeInTheDocument();
    });
  });

  it('still renders Gini coefficients', async () => {
    render(<InterferenceTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('0.61')).toBeInTheDocument();
    });

    expect(screen.getByText('0.58')).toBeInTheDocument();
  });

  it('still renders spillover titles table', async () => {
    render(<InterferenceTab experimentId="11111111-1111-1111-1111-111111111111" />);

    await waitFor(() => {
      expect(screen.getByText('title-1234')).toBeInTheDocument();
    });

    expect(screen.getByText('title-5678')).toBeInTheDocument();
    expect(screen.getByText('title-9012')).toBeInTheDocument();
  });
});
