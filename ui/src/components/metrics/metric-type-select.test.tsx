/**
 * Tests for MetricTypeSelect (ADR-026 Phase 3, Task D1 + L6 phase 3.B).
 *
 * Verifies the CUSTOM deprecation surface in the metric-create form:
 *   - Phase 3.A (default state, flag unset/false):
 *     - Label rewrite ("Custom SQL (deprecated)")
 *     - Description rewrite (migration-guide pointer)
 *     - Inline AlertTriangle warning icon shown only when CUSTOM is selected
 *     - Icon absent for non-CUSTOM types (MEAN, FILTERED_MEAN)
 *     - onChange still propagates the selected MetricType
 *   - L6 phase 3.B (sunset flag `m6.metric_type.custom.hidden` = true):
 *     - CUSTOM option is removed from the <select> entirely
 *     - All other options remain visible
 */

import { afterEach, beforeEach, describe, test, expect, vi } from 'vitest';
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

  /**
   * L6 phase 3.B sunset gate.
   *
   * The flag key `m6.metric_type.custom.hidden` is exposed today as the
   * Next.js public env var NEXT_PUBLIC_METRIC_TYPE_CUSTOM_HIDDEN. When set
   * to "true", the CUSTOM option must be filtered out of the create-form
   * <select> entirely (per the locked plan, ADR-026 Phase 3 L6).
   *
   * Operators flip this flag after observing 4 weeks of zero CUSTOM creates
   * via the `metric_definition_custom_created_total` counter emitted from M5.
   * Flipping it begins the 2-cycle countdown for #602 (proto enum removal).
   */
  describe('when m6.metric_type.custom.hidden flag is true', () => {
    const ENV_KEY = 'NEXT_PUBLIC_METRIC_TYPE_CUSTOM_HIDDEN';
    let previous: string | undefined;

    beforeEach(() => {
      previous = process.env[ENV_KEY];
      process.env[ENV_KEY] = 'true';
    });

    afterEach(() => {
      if (previous === undefined) {
        delete process.env[ENV_KEY];
      } else {
        process.env[ENV_KEY] = previous;
      }
    });

    test('omits the CUSTOM option from the <select>', () => {
      render(<MetricTypeSelect value="MEAN" onChange={() => {}} />);
      expect(
        screen.queryByRole('option', { name: /custom sql.*deprecated/i }),
      ).not.toBeInTheDocument();
    });

    test('still renders the other metric-type options', () => {
      render(<MetricTypeSelect value="MEAN" onChange={() => {}} />);
      expect(screen.getByRole('option', { name: 'Mean' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Filtered Mean' })).toBeInTheDocument();
      expect(
        screen.getByRole('option', { name: 'MetricQL expression' }),
      ).toBeInTheDocument();
    });

    // Devin PR #603 📝 future-caller defensive-gate regression. The current
    // `/metrics/new` caller can't reach `value='CUSTOM'` when the flag is on
    // (CUSTOM is filtered out, so the <select> can't be set to it), but a
    // future edit-context caller mounting on an existing CUSTOM metric
    // would. Without the `&& !isCustomHidden()` gate, the deprecation icon
    // would render next to a <select> value with no matching <option>.
    test('does NOT render the deprecation icon when value=CUSTOM and flag is on (future-caller guard)', () => {
      render(<MetricTypeSelect value="CUSTOM" onChange={() => {}} />);
      expect(
        screen.queryByTestId('metric-type-deprecated-icon'),
      ).not.toBeInTheDocument();
    });
  });

  /**
   * Regression guard: when the env var is explicitly "false" (or any value
   * other than the literal string "true"), the gate is OFF and CUSTOM
   * remains visible with its Phase 3.A deprecated label.
   */
  describe('when m6.metric_type.custom.hidden flag is false', () => {
    const ENV_KEY = 'NEXT_PUBLIC_METRIC_TYPE_CUSTOM_HIDDEN';
    let previous: string | undefined;

    beforeEach(() => {
      previous = process.env[ENV_KEY];
      process.env[ENV_KEY] = 'false';
    });

    afterEach(() => {
      if (previous === undefined) {
        delete process.env[ENV_KEY];
      } else {
        process.env[ENV_KEY] = previous;
      }
    });

    test('keeps the deprecated CUSTOM option visible', () => {
      render(<MetricTypeSelect value="MEAN" onChange={() => {}} />);
      expect(
        screen.getByRole('option', { name: /custom sql.*deprecated/i }),
      ).toBeInTheDocument();
    });
  });
});
