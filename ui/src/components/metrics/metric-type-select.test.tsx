/**
 * Tests for MetricTypeSelect (ADR-026 Phase 3, Task D1).
 *
 * Verifies the CUSTOM deprecation surface in the metric-create form:
 *   - Label rewrite ("Custom SQL (deprecated)")
 *   - Description rewrite (migration-guide pointer)
 *   - Inline AlertTriangle warning icon shown only when CUSTOM is selected
 *   - Icon absent for non-CUSTOM types (MEAN, FILTERED_MEAN)
 *   - onChange still propagates the selected MetricType
 */

import { describe, test, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MetricTypeSelect } from './metric-type-select';

describe('MetricTypeSelect', () => {
  test('renders the deprecated label for CUSTOM in the option list', () => {
    render(<MetricTypeSelect value="MEAN" onChange={() => {}} />);
    const customOption = screen.getByRole('option', { name: /custom sql.*deprecated/i });
    expect(customOption).toBeInTheDocument();
  });

  test('shows the deprecation warning icon and message when CUSTOM is selected', () => {
    render(<MetricTypeSelect value="CUSTOM" onChange={() => {}} />);
    expect(screen.getByTestId('metric-type-deprecated-icon')).toBeInTheDocument();
    expect(screen.getByTestId('metric-type-description')).toHaveTextContent(/deprecated/i);
    expect(screen.getByTestId('metric-type-description')).toHaveTextContent(/migration guide/i);
  });

  test('does NOT show the deprecation icon for non-CUSTOM types', () => {
    render(<MetricTypeSelect value="MEAN" onChange={() => {}} />);
    expect(screen.queryByTestId('metric-type-deprecated-icon')).not.toBeInTheDocument();
  });

  test('does NOT show the deprecation icon for FILTERED_MEAN', () => {
    render(<MetricTypeSelect value="FILTERED_MEAN" onChange={() => {}} />);
    expect(screen.queryByTestId('metric-type-deprecated-icon')).not.toBeInTheDocument();
  });

  test('fires onChange with the new MetricType when the user picks an option', () => {
    const onChange = vi.fn();
    render(<MetricTypeSelect value="MEAN" onChange={onChange} />);
    fireEvent.change(screen.getByTestId('metric-type-select'), { target: { value: 'CUSTOM' } });
    expect(onChange).toHaveBeenCalledWith('CUSTOM');
  });
});
