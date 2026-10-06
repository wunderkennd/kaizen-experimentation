import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { OperandPicker } from './operand-picker';
import * as api from '@/lib/api';
import type { CompositeOperand, MetricDefinition } from '@/lib/types';

vi.mock('@/lib/api', () => ({
  listMetricDefinitions: vi.fn(),
}));

const mockMetrics: MetricDefinition[] = [
  { metricId: 'metric-1', name: 'Conversion Rate', description: 'CR', type: 'WINDOWED_COUNT', sourceEventType: 'click', lowerIsBetter: false, isQoeMetric: false },
  { metricId: 'metric-2', name: 'Revenue', description: 'Rev', type: 'FILTERED_MEAN', sourceEventType: 'purchase', lowerIsBetter: false, isQoeMetric: false },
  { metricId: 'metric-3', name: 'Retention Rate', description: 'Ret', type: 'WINDOWED_COUNT', sourceEventType: 'session', lowerIsBetter: false, isQoeMetric: false },
];

describe('OperandPicker', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listMetricDefinitions).mockResolvedValue({ metrics: mockMetrics, nextPageToken: '' });
  });

  it('renders input with keyboard shortcut hint and clear button when query exists', async () => {
    const onChange = vi.fn();
    render(<OperandPicker value={[]} onChange={onChange} showWeights={false} />);

    await waitFor(() => expect(screen.getByPlaceholderText('Search metric ID or name…')).toBeInTheDocument());

    const input = screen.getByRole('textbox', { name: 'Search operands' });
    expect(screen.getByText('/')).toBeInTheDocument();

    await userEvent.type(input, 'Conversion');
    expect(input).toHaveValue('Conversion');

    const clearBtn = screen.getByTestId('clear-search-button');
    expect(clearBtn).toBeInTheDocument();

    await userEvent.click(clearBtn);
    expect(input).toHaveValue('');
    expect(input).toHaveFocus();
  });

  it('renders Clear all button when multiple operands selected and clears on click', async () => {
    const onChange = vi.fn();
    const initialValue: CompositeOperand[] = [
      { metricId: 'metric-1', weight: 1.0 },
      { metricId: 'metric-2', weight: 1.0 },
    ];

    render(<OperandPicker value={initialValue} onChange={onChange} showWeights={true} />);

    await waitFor(() => expect(screen.getByTestId('selected-operands')).toBeInTheDocument());

    expect(screen.getByText('Selected Operands (2)')).toBeInTheDocument();
    const clearAllBtn = screen.getByTestId('clear-all-operands-button');
    expect(clearAllBtn).toBeInTheDocument();

    await userEvent.click(clearAllBtn);
    expect(onChange).toHaveBeenCalledWith([]);
  });

  it('does not render Clear all button when only 1 operand selected', async () => {
    const onChange = vi.fn();
    const initialValue: CompositeOperand[] = [
      { metricId: 'metric-1', weight: 1.0 },
    ];

    render(<OperandPicker value={initialValue} onChange={onChange} showWeights={false} />);

    await waitFor(() => expect(screen.getByTestId('selected-operands')).toBeInTheDocument());
    expect(screen.queryByTestId('clear-all-operands-button')).not.toBeInTheDocument();
  });

  it('focuses search input when slash key is pressed', async () => {
    const onChange = vi.fn();
    render(<OperandPicker value={[]} onChange={onChange} showWeights={false} />);

    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Search operands' })).toBeInTheDocument());

    const input = screen.getByRole('textbox', { name: 'Search operands' });
    expect(input).not.toHaveFocus();

    fireEvent.keyDown(window, { key: '/' });
    expect(input).toHaveFocus();
  });
});
