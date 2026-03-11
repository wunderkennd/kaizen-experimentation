import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { LayerAllocationChart } from '@/components/layer-allocation-chart';

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
    Tooltip: Noop,
    Cell: Noop,
  };
});

describe('LayerAllocationChart', () => {
  it('renders chart with allocations for RUNNING experiment', async () => {
    render(
      <LayerAllocationChart
        layerId="layer-homepage"
        currentExperimentId="11111111-1111-1111-1111-111111111111"
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Homepage')).toBeInTheDocument();
    });

    // Shows total buckets
    expect(screen.getByText('10,000 total buckets')).toBeInTheDocument();

    // Shows legend table
    const legendTable = screen.getByTestId('layer-legend-table');
    expect(legendTable).toBeInTheDocument();

    // Current experiment has a "current" badge
    expect(screen.getByText('current')).toBeInTheDocument();

    // Bucket range displayed
    expect(screen.getByText('0 - 4,999')).toBeInTheDocument();

    // Traffic percentage (appears for both allocated and unallocated)
    expect(screen.getAllByText('50.0%').length).toBeGreaterThanOrEqual(1);
  });

  it('shows empty state for DRAFT experiment with no allocations', async () => {
    render(
      <LayerAllocationChart
        layerId="layer-playback"
        currentExperimentId="22222222-2222-2222-2222-222222222222"
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId('layer-empty-state')).toBeInTheDocument();
    });

    expect(
      screen.getByText('No bucket allocations yet. Allocations are created when the experiment starts.'),
    ).toBeInTheDocument();
  });

  it('shows legend table with bucket range and percentage for multi-allocation layer', async () => {
    // layer-search has one active allocation (alloc-2, archived one filtered out by default)
    render(
      <LayerAllocationChart
        layerId="layer-search"
        currentExperimentId="33333333-3333-3333-3333-333333333333"
      />,
    );

    await waitFor(() => {
      expect(screen.getByText('Search')).toBeInTheDocument();
    });

    // The active allocation: buckets 0-4999 = 50.0%
    expect(screen.getByText('0 - 4,999')).toBeInTheDocument();
    expect(screen.getAllByText('50.0%').length).toBeGreaterThanOrEqual(1);

    // The unallocated section should also show
    expect(screen.getByText('Unallocated')).toBeInTheDocument();
  });

  it('handles API error gracefully', async () => {
    render(
      <LayerAllocationChart
        layerId="nonexistent-layer"
        currentExperimentId="11111111-1111-1111-1111-111111111111"
      />,
    );

    await waitFor(() => {
      expect(screen.getByText(/Failed to load layer allocation/)).toBeInTheDocument();
    });
  });

  it('renders accessibility label on chart', async () => {
    render(
      <LayerAllocationChart
        layerId="layer-homepage"
        currentExperimentId="11111111-1111-1111-1111-111111111111"
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole('img', { name: /Bucket allocation for Homepage layer/ })).toBeInTheDocument();
    });
  });
});
