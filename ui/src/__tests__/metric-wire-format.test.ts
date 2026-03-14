/**
 * Metric definition wire-format contract tests.
 *
 * Validates that listMetricDefinitions() + adaptMetricDefinition() correctly
 * handles the real Agent-5 ConnectRPC JSON wire format:
 *   - METRIC_TYPE_ enum prefix stripping
 *   - Proto3 zero-value omission (false booleans, 0 numbers absent)
 *   - Optional fields (ratio numerator/denominator, percentile, custom SQL)
 *   - Pagination (page_size, page_token, next_page_token)
 *   - typeFilter sent with METRIC_TYPE_ prefix
 */
import { describe, it, expect } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { listMetricDefinitions } from '@/lib/api';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';

describe('Metric definition wire format', () => {
  it('strips METRIC_TYPE_ prefix from all enum values', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () =>
        HttpResponse.json({
          metrics: [
            { metricId: 'm1', name: 'Mean Metric', type: 'METRIC_TYPE_MEAN', sourceEventType: 'event' },
            { metricId: 'm2', name: 'Proportion Metric', type: 'METRIC_TYPE_PROPORTION', sourceEventType: 'event' },
            { metricId: 'm3', name: 'Ratio Metric', type: 'METRIC_TYPE_RATIO', sourceEventType: 'event' },
            { metricId: 'm4', name: 'Count Metric', type: 'METRIC_TYPE_COUNT', sourceEventType: 'event' },
            { metricId: 'm5', name: 'Percentile Metric', type: 'METRIC_TYPE_PERCENTILE', sourceEventType: 'event' },
            { metricId: 'm6', name: 'Custom Metric', type: 'METRIC_TYPE_CUSTOM', sourceEventType: 'event' },
          ],
          nextPageToken: '',
        }),
      ),
    );

    const result = await listMetricDefinitions();
    expect(result.metrics.map((m) => m.type)).toEqual([
      'MEAN', 'PROPORTION', 'RATIO', 'COUNT', 'PERCENTILE', 'CUSTOM',
    ]);
  });

  it('handles proto3 zero-value omission — false booleans default correctly', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () =>
        HttpResponse.json({
          metrics: [
            {
              metricId: 'zero-vals',
              name: 'Zero Value Test',
              type: 'METRIC_TYPE_MEAN',
              sourceEventType: 'event',
              // Proto3: lowerIsBetter=false and isQoeMetric=false are OMITTED
            },
          ],
          nextPageToken: '',
        }),
      ),
    );

    const result = await listMetricDefinitions();
    const m = result.metrics[0];
    expect(m.lowerIsBetter).toBe(false);
    expect(m.isQoeMetric).toBe(false);
    expect(m.description).toBe('');
  });

  it('preserves true boolean values when present', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () =>
        HttpResponse.json({
          metrics: [
            {
              metricId: 'true-vals',
              name: 'True Value Test',
              type: 'METRIC_TYPE_RATIO',
              sourceEventType: 'qoe',
              lowerIsBetter: true,
              isQoeMetric: true,
            },
          ],
          nextPageToken: '',
        }),
      ),
    );

    const result = await listMetricDefinitions();
    const m = result.metrics[0];
    expect(m.lowerIsBetter).toBe(true);
    expect(m.isQoeMetric).toBe(true);
  });

  it('maps all 14 MetricDefinition proto fields', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () =>
        HttpResponse.json({
          metrics: [
            {
              metricId: 'full-metric',
              name: 'Full Metric',
              description: 'All fields populated',
              type: 'METRIC_TYPE_RATIO',
              sourceEventType: 'composite',
              numeratorEventType: 'revenue',
              denominatorEventType: 'sessions',
              percentile: 0.95,
              customSql: 'SELECT 1',
              lowerIsBetter: true,
              surrogateTargetMetricId: 'long_term_revenue',
              isQoeMetric: true,
              cupedCovariateMetricId: 'pre_revenue',
              minimumDetectableEffect: 0.03,
            },
          ],
          nextPageToken: '',
        }),
      ),
    );

    const result = await listMetricDefinitions();
    const m = result.metrics[0];
    expect(m).toEqual({
      metricId: 'full-metric',
      name: 'Full Metric',
      description: 'All fields populated',
      type: 'RATIO',
      sourceEventType: 'composite',
      numeratorEventType: 'revenue',
      denominatorEventType: 'sessions',
      percentile: 0.95,
      customSql: 'SELECT 1',
      lowerIsBetter: true,
      surrogateTargetMetricId: 'long_term_revenue',
      isQoeMetric: true,
      cupedCovariateMetricId: 'pre_revenue',
      minimumDetectableEffect: 0.03,
    });
  });

  it('handles empty metrics list', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () =>
        HttpResponse.json({ metrics: [], nextPageToken: '' }),
      ),
    );

    const result = await listMetricDefinitions();
    expect(result.metrics).toEqual([]);
    expect(result.nextPageToken).toBe('');
  });

  it('handles missing metrics field (proto3 empty repeated = absent)', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () =>
        HttpResponse.json({}),
      ),
    );

    const result = await listMetricDefinitions();
    expect(result.metrics).toEqual([]);
    expect(result.nextPageToken).toBe('');
  });

  it('pagination: passes pageSize and pageToken, reads nextPageToken', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, async ({ request }) => {
        const body = await request.json() as Record<string, unknown>;
        expect(body.pageSize).toBe(2);
        expect(body.pageToken).toBe('abc');
        return HttpResponse.json({
          metrics: [
            { metricId: 'p1', name: 'Page 1', type: 'METRIC_TYPE_MEAN', sourceEventType: 'e' },
          ],
          nextPageToken: 'def',
        });
      }),
    );

    const result = await listMetricDefinitions({ pageSize: 2, pageToken: 'abc' });
    expect(result.metrics).toHaveLength(1);
    expect(result.nextPageToken).toBe('def');
  });

  it('typeFilter sends METRIC_TYPE_ prefixed value in request body', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, async ({ request }) => {
        const body = await request.json() as Record<string, unknown>;
        expect(body.typeFilter).toBe('METRIC_TYPE_PROPORTION');
        return HttpResponse.json({
          metrics: [
            { metricId: 'p1', name: 'Proportion', type: 'METRIC_TYPE_PROPORTION', sourceEventType: 'e' },
          ],
          nextPageToken: '',
        });
      }),
    );

    const result = await listMetricDefinitions({ typeFilter: 'PROPORTION' });
    expect(result.metrics[0].type).toBe('PROPORTION');
  });

  it('default MSW handler returns proto wire format and adapter handles it', async () => {
    // Uses the default handler — validates the full roundtrip from seed data
    const result = await listMetricDefinitions();
    expect(result.metrics.length).toBe(12);

    // Verify enum prefix was stripped
    for (const m of result.metrics) {
      expect(m.type).not.toContain('METRIC_TYPE_');
      expect(['MEAN', 'PROPORTION', 'RATIO', 'COUNT', 'PERCENTILE', 'CUSTOM']).toContain(m.type);
    }

    // Verify zero-value booleans default to false
    const streamStartRate = result.metrics.find((m) => m.metricId === 'stream_start_rate')!;
    expect(streamStartRate.lowerIsBetter).toBe(false);
    expect(streamStartRate.isQoeMetric).toBe(false);

    // Verify true booleans preserved
    const rebufferRate = result.metrics.find((m) => m.metricId === 'rebuffer_rate')!;
    expect(rebufferRate.lowerIsBetter).toBe(true);
    expect(rebufferRate.isQoeMetric).toBe(true);
  });

  it('handles METRIC_TYPE_UNSPECIFIED by passing through as "UNSPECIFIED"', async () => {
    server.use(
      http.post(`${MGMT_SVC}/ListMetricDefinitions`, () =>
        HttpResponse.json({
          metrics: [
            { metricId: 'unspec', name: 'Unspecified', type: 'METRIC_TYPE_UNSPECIFIED', sourceEventType: '' },
          ],
          nextPageToken: '',
        }),
      ),
    );

    const result = await listMetricDefinitions();
    expect(result.metrics[0].type).toBe('UNSPECIFIED');
  });
});
