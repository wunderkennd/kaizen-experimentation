/**
 * Agent-4 (M4a) ↔ Agent-6 (M6) wire-format contract tests.
 *
 * These tests simulate the EXACT JSON payloads that Agent-4's Rust analysis
 * service will send over ConnectRPC, and verify that Agent-6's API client
 * correctly parses them into UI types.
 *
 * Key wire-format concerns validated here:
 *   1. Proto enum prefixes (LIFECYCLE_SEGMENT_TRIAL, SEQUENTIAL_METHOD_MSPRT)
 *   2. Proto3 zero-value omission (0, false, "", [] omitted from JSON)
 *   3. int64 serialized as strings in proto3 JSON (sample_size: "12000")
 *   4. Timestamp fields as RFC 3339 strings
 *   5. Flat response envelopes (analysis responses have no wrapper)
 *   6. SurrogateProjection field mapping (proto vs UI type mismatch)
 *   7. Map<string, int64> in SRM counts
 *
 * Proto source: proto/experimentation/analysis/v1/analysis_service.proto
 * UI types:     ui/src/lib/types.ts
 * API client:   ui/src/lib/api.ts
 */
import { describe, it, expect } from 'vitest';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import {
  getAnalysisResult,
  getNoveltyAnalysis,
  getInterferenceAnalysis,
  getInterleavingAnalysis,
  getCateAnalysis,
  getGstTrajectory,
  getCumulativeHoldoutResult,
  getQoeDashboard,
  getBanditDashboard,
  RpcError,
} from '@/lib/api';

const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';
const BANDIT_SVC = '*/experimentation.bandit.v1.BanditPolicyService';

// ────────────────────────────────────────────────────────────────────────────
// 1. GetAnalysisResult — proto wire format
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: GetAnalysisResult', () => {
  it('parses a realistic proto3 JSON AnalysisResult', async () => {
    // This is what tonic + prost serialize: camelCase, Timestamp as string,
    // enums as string names, int64 as strings in maps.
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: '11111111-1111-1111-1111-111111111111',
          metricResults: [
            {
              metricId: 'click_through_rate',
              variantId: 'v1-treatment',
              controlMean: 0.124,
              treatmentMean: 0.138,
              absoluteEffect: 0.014,
              relativeEffect: 0.1129,
              ciLower: 0.003,
              ciUpper: 0.025,
              pValue: 0.008,
              isSignificant: true,
              cupedAdjustedEffect: 0.013,
              cupedCiLower: 0.005,
              cupedCiUpper: 0.021,
              varianceReductionPct: 32,
              sequentialResult: {
                boundaryCrossed: true,
                alphaSpent: 0.032,
                alphaRemaining: 0.018,
                currentLook: 3,
                adjustedPValue: 0.012,
              },
              segmentResults: [
                {
                  segment: 'LIFECYCLE_SEGMENT_TRIAL',
                  effect: 0.032,
                  ciLower: 0.010,
                  ciUpper: 0.054,
                  pValue: 0.004,
                  sampleSize: '8350', // int64 as string in proto3 JSON
                },
                {
                  segment: 'LIFECYCLE_SEGMENT_MATURE',
                  effect: 0.004,
                  ciLower: -0.008,
                  ciUpper: 0.016,
                  pValue: 0.51,
                  sampleSize: '31700',
                },
              ],
              sessionLevelResult: {
                naiveSe: 0.0031,
                clusteredSe: 0.0045,
                designEffect: 2.1,
                naivePValue: 0.003,
                clusteredPValue: 0.039,
              },
            },
          ],
          srmResult: {
            chiSquared: 0.42,
            pValue: 0.517,
            isMismatch: false,
            observedCounts: { 'v1-control': '50102', 'v1-treatment': '49898' },
            expectedCounts: { 'v1-control': '50000', 'v1-treatment': '50000' },
          },
          surrogateProjections: [
            {
              // Proto SurrogateProjection fields
              experimentId: '11111111-1111-1111-1111-111111111111',
              variantId: 'v1-treatment',
              modelId: 'surrogate-homepage-ltv',
              projectedEffect: 0.008,
              projectionCiLower: 0.002,
              projectionCiUpper: 0.014,
              calibrationRSquared: 0.78,
              computedAt: '2026-03-05T12:00:00Z',
            },
          ],
          cochranQPValue: 0.006,
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('11111111-1111-1111-1111-111111111111');

    // Basic structure
    expect(result.experimentId).toBe('11111111-1111-1111-1111-111111111111');
    expect(result.metricResults).toHaveLength(1);
    expect(result.computedAt).toBe('2026-03-05T12:00:00Z');

    // Metric result fields
    const mr = result.metricResults[0];
    expect(mr.metricId).toBe('click_through_rate');
    expect(mr.isSignificant).toBe(true);
    expect(mr.pValue).toBe(0.008);
    expect(mr.cupedAdjustedEffect).toBe(0.013);
    expect(mr.varianceReductionPct).toBe(32);

    // Sequential result nested
    expect(mr.sequentialResult).toBeDefined();
    expect(mr.sequentialResult!.boundaryCrossed).toBe(true);
    expect(mr.sequentialResult!.currentLook).toBe(3);

    // Session-level result nested
    expect(mr.sessionLevelResult).toBeDefined();
    expect(mr.sessionLevelResult!.designEffect).toBe(2.1);
    expect(mr.sessionLevelResult!.clusteredPValue).toBe(0.039);

    // SRM result — int64 counts coerced to numbers by adapter
    expect(result.srmResult.isMismatch).toBe(false);
    expect(result.srmResult.chiSquared).toBe(0.42);
    expect(typeof result.srmResult.observedCounts['v1-control']).toBe('number');
    expect(result.srmResult.observedCounts['v1-control']).toBe(50102);

    // cochranQPValue — proto field now in UI type
    expect(result.cochranQPValue).toBe(0.006);

    // segmentResults — adapted with enum prefix stripping and int64 coercion
    expect(mr.segmentResults).toHaveLength(2);
    expect(mr.segmentResults![0].segment).toBe('TRIAL'); // stripped LIFECYCLE_SEGMENT_
    expect(mr.segmentResults![0].sampleSize).toBe(8350); // coerced from string
    expect(typeof mr.segmentResults![0].sampleSize).toBe('number');
    expect(mr.segmentResults![1].segment).toBe('MATURE');
    expect(mr.segmentResults![1].sampleSize).toBe(31700);

    // SurrogateProjection — adapted from proto fields
    expect(result.surrogateProjections).toHaveLength(1);
    const sp = result.surrogateProjections![0];
    expect(sp.metricId).toBe('surrogate-homepage-ltv'); // adapted from modelId
    expect(sp.surrogateMetricId).toBe('v1-treatment'); // adapted from variantId
    expect(sp.modelId).toBe('surrogate-homepage-ltv'); // proto field preserved
    expect(sp.projectedEffect).toBe(0.008);
  });

  it('handles proto3 zero-value omission gracefully', async () => {
    // Proto3 omits fields with default values: 0.0, false, "", []
    // This simulates an experiment with no significant results where many
    // fields are at their zero values and thus absent from the JSON.
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'zero-test',
          // metricResults omitted (empty array → omitted by proto3)
          srmResult: {
            // chiSquared: 0.0 → omitted
            // pValue: 0.0 → omitted
            // isMismatch: false → omitted
          },
          // cochranQPValue: 0.0 → omitted
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('zero-test');
    expect(result.experimentId).toBe('zero-test');
    // metricResults: empty array after adapter (proto3 omits empty arrays → undefined → adapter defaults to [])
    expect(result.metricResults).toEqual([]);
    // SRM fields default to zero/false after adapter
    expect(result.srmResult.chiSquared).toBe(0);
    expect(result.srmResult.pValue).toBe(0);
    expect(result.srmResult.isMismatch).toBe(false);
    // cochranQPValue: 0.0 omitted by proto3 → undefined
    expect(result.cochranQPValue).toBeUndefined();
  });

  it('handles int64 string coercion in SRM counts', async () => {
    // Proto3 JSON serializes int64 as strings. ConnectRPC might send either
    // string or number depending on implementation. Our UI must handle both.
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'int64-test',
          metricResults: [],
          srmResult: {
            chiSquared: 14.82,
            pValue: 0.0001,
            isMismatch: true,
            // int64 as strings — real proto3 JSON format
            observedCounts: { 'v1-control': '52300', 'v1-treatment': '47700' },
            expectedCounts: { 'v1-control': '50000', 'v1-treatment': '50000' },
          },
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('int64-test');
    expect(result.srmResult.isMismatch).toBe(true);
    expect(result.srmResult.observedCounts).toBeDefined();
    // Adapter coerces int64 strings to numbers
    const controlCount = result.srmResult.observedCounts['v1-control'];
    expect(controlCount).toBe(52300);
    expect(typeof controlCount).toBe('number');
    expect(result.srmResult.expectedCounts['v1-control']).toBe(50000);
    expect(typeof result.srmResult.expectedCounts['v1-control']).toBe('number');
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 2. GetNoveltyAnalysis — proto wire format
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: GetNoveltyAnalysis', () => {
  it('parses a realistic proto3 NoveltyAnalysisResult', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, () =>
        HttpResponse.json({
          experimentId: '11111111-1111-1111-1111-111111111111',
          metricId: 'click_through_rate',
          noveltyDetected: true,
          rawTreatmentEffect: 0.014,
          projectedSteadyStateEffect: 0.009,
          noveltyAmplitude: 0.018,
          decayConstantDays: 4.2,
          isStabilized: false,
          daysUntilProjectedStability: 6,
          // NOTE: dailyEffects NOT in proto — real API won't send this field
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getNoveltyAnalysis('11111111-1111-1111-1111-111111111111');
    expect(result.noveltyDetected).toBe(true);
    expect(result.rawTreatmentEffect).toBe(0.014);
    expect(result.projectedSteadyStateEffect).toBe(0.009);
    expect(result.decayConstantDays).toBe(4.2);
    expect(result.isStabilized).toBe(false);
    expect(result.daysUntilProjectedStability).toBe(6);
    expect(result.computedAt).toBe('2026-03-05T12:00:00Z');
    // dailyEffects will be undefined from real proto API
    expect(result.dailyEffects).toBeUndefined();
  });

  it('handles stabilized experiment with proto3 zero omission', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, () =>
        HttpResponse.json({
          experimentId: 'stable-1',
          metricId: 'watch_time',
          // noveltyDetected: false → omitted by proto3
          rawTreatmentEffect: 0.008,
          projectedSteadyStateEffect: 0.008,
          // noveltyAmplitude: 0.0 → omitted
          // decayConstantDays: 0.0 → omitted
          isStabilized: true,
          // daysUntilProjectedStability: 0 → omitted
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getNoveltyAnalysis('stable-1');
    expect(result.noveltyDetected).toBeUndefined(); // proto3 omits false
    expect(result.noveltyAmplitude).toBeUndefined(); // proto3 omits 0.0
    expect(result.isStabilized).toBe(true);
  });

  it('returns 404 for experiments without novelty data', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, () =>
        HttpResponse.json(
          { code: 'not_found', message: 'No novelty data' },
          { status: 404 },
        ),
      ),
    );

    await expect(getNoveltyAnalysis('missing')).rejects.toThrow(RpcError);
    try {
      await getNoveltyAnalysis('missing');
    } catch (e) {
      expect((e as RpcError).status).toBe(404);
    }
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 3. GetInterferenceAnalysis — proto wire format
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: GetInterferenceAnalysis', () => {
  it('parses InterferenceAnalysisResult from proto', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetInterferenceAnalysis`, () =>
        HttpResponse.json({
          experimentId: '11111111-1111-1111-1111-111111111111',
          interferenceDetected: true,
          jensenShannonDivergence: 0.042,
          jaccardSimilarityTop100: 0.73,
          treatmentGiniCoefficient: 0.61,
          controlGiniCoefficient: 0.58,
          treatmentCatalogCoverage: 0.34,
          controlCatalogCoverage: 0.31,
          spilloverTitles: [
            {
              contentId: 'title-1234',
              treatmentWatchRate: 0.082,
              controlWatchRate: 0.041,
              pValue: 0.002,
            },
          ],
          // NOTE: treatmentLorenzCurve/controlLorenzCurve NOT in proto
          // They exist in UI types but won't come from real M4a service
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getInterferenceAnalysis('11111111-1111-1111-1111-111111111111');
    expect(result.interferenceDetected).toBe(true);
    expect(result.jensenShannonDivergence).toBe(0.042);
    expect(result.spilloverTitles).toHaveLength(1);
    expect(result.spilloverTitles[0].contentId).toBe('title-1234');
    // Lorenz curves will be undefined from real API
    expect(result.treatmentLorenzCurve).toBeUndefined();
    expect(result.controlLorenzCurve).toBeUndefined();
  });

  it('handles no interference detected (many zero values)', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetInterferenceAnalysis`, () =>
        HttpResponse.json({
          experimentId: 'clean-1',
          // interferenceDetected: false → omitted
          jensenShannonDivergence: 0.003,
          jaccardSimilarityTop100: 0.95,
          treatmentGiniCoefficient: 0.45,
          controlGiniCoefficient: 0.44,
          treatmentCatalogCoverage: 0.42,
          controlCatalogCoverage: 0.41,
          // spilloverTitles: [] → omitted
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getInterferenceAnalysis('clean-1');
    expect(result.interferenceDetected).toBeUndefined(); // proto3 omits false
    expect(result.spilloverTitles).toBeUndefined(); // proto3 omits empty repeated
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 4. GetInterleavingAnalysis — proto wire format
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: GetInterleavingAnalysis', () => {
  it('parses InterleavingAnalysisResult with maps', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetInterleavingAnalysis`, () =>
        HttpResponse.json({
          experimentId: '33333333-3333-3333-3333-333333333333',
          algorithmWinRates: { bm25_baseline: 0.42, semantic_search: 0.58 },
          signTestPValue: 0.003,
          algorithmStrengths: [
            { algorithmId: 'bm25_baseline', strength: 0.45, ciLower: 0.39, ciUpper: 0.51 },
            { algorithmId: 'semantic_search', strength: 0.55, ciLower: 0.49, ciUpper: 0.61 },
          ],
          positionAnalyses: [
            {
              position: 1,
              algorithmEngagementRates: { bm25_baseline: 0.31, semantic_search: 0.38 },
            },
          ],
          computedAt: '2026-03-05T14:35:00Z',
        }),
      ),
    );

    const result = await getInterleavingAnalysis('33333333-3333-3333-3333-333333333333');
    expect(result.algorithmWinRates.bm25_baseline).toBe(0.42);
    expect(result.algorithmWinRates.semantic_search).toBe(0.58);
    expect(result.signTestPValue).toBe(0.003);
    expect(result.algorithmStrengths).toHaveLength(2);
    expect(result.positionAnalyses[0].position).toBe(1);
    expect(result.positionAnalyses[0].algorithmEngagementRates.semantic_search).toBe(0.38);
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 5. CATE — lifecycle segment enum prefix stripping
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: GetCateAnalysis — enum prefix stripping', () => {
  it('strips LIFECYCLE_SEGMENT_ prefix from segment enums', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetCateAnalysis`, () =>
        HttpResponse.json({
          experimentId: 'cate-1',
          metricId: 'click_through_rate',
          globalAte: 0.014,
          globalSe: 0.0042,
          globalCiLower: 0.006,
          globalCiUpper: 0.022,
          globalPValue: 0.001,
          subgroupEffects: [
            {
              segment: 'LIFECYCLE_SEGMENT_TRIAL',
              effect: 0.032,
              se: 0.011,
              ciLower: 0.010,
              ciUpper: 0.054,
              pValueRaw: 0.004,
              pValueAdjusted: 0.012,
              isSignificant: true,
              nControl: 4200,
              nTreatment: 4150,
              controlMean: 0.108,
              treatmentMean: 0.140,
            },
            {
              segment: 'LIFECYCLE_SEGMENT_NEW',
              effect: 0.021,
              se: 0.008,
              ciLower: 0.005,
              ciUpper: 0.037,
              pValueRaw: 0.009,
              pValueAdjusted: 0.018,
              isSignificant: true,
              nControl: 8500,
              nTreatment: 8400,
              controlMean: 0.118,
              treatmentMean: 0.139,
            },
            {
              segment: 'LIFECYCLE_SEGMENT_ESTABLISHED',
              effect: 0.008,
              se: 0.005,
              ciLower: -0.002,
              ciUpper: 0.018,
              pValueRaw: 0.11,
              pValueAdjusted: 0.165,
              // isSignificant: false → omitted by proto3
              nControl: 18200,
              nTreatment: 18100,
              controlMean: 0.129,
              treatmentMean: 0.137,
            },
            {
              segment: 'LIFECYCLE_SEGMENT_MATURE',
              effect: 0.004,
              se: 0.006,
              ciLower: -0.008,
              ciUpper: 0.016,
              pValueRaw: 0.51,
              pValueAdjusted: 0.51,
              // isSignificant: false → omitted
              nControl: 15800,
              nTreatment: 15900,
              controlMean: 0.131,
              treatmentMean: 0.135,
            },
            {
              segment: 'LIFECYCLE_SEGMENT_AT_RISK',
              effect: -0.002,
              se: 0.012,
              ciLower: -0.026,
              ciUpper: 0.022,
              pValueRaw: 0.87,
              pValueAdjusted: 0.87,
              nControl: 2800,
              nTreatment: 2750,
              controlMean: 0.095,
              treatmentMean: 0.093,
            },
            {
              segment: 'LIFECYCLE_SEGMENT_WINBACK',
              effect: 0.015,
              se: 0.014,
              ciLower: -0.013,
              ciUpper: 0.043,
              pValueRaw: 0.29,
              pValueAdjusted: 0.35,
              nControl: 1600,
              nTreatment: 1580,
              controlMean: 0.102,
              treatmentMean: 0.117,
            },
          ],
          heterogeneity: {
            qStatistic: 12.4,
            df: 5,
            pValue: 0.03,
            iSquared: 59.7,
            heterogeneityDetected: true,
          },
          nSubgroups: 6,
          fdrThreshold: 0.05,
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getCateAnalysis('cate-1');

    // All 6 segments should have prefixes stripped
    const segments = result.subgroupEffects.map((sg) => sg.segment);
    expect(segments).toEqual([
      'TRIAL', 'NEW', 'ESTABLISHED', 'MATURE', 'AT_RISK', 'WINBACK',
    ]);

    // Verify the stripping didn't break other fields
    expect(result.subgroupEffects[0].effect).toBe(0.032);
    expect(result.subgroupEffects[0].nControl).toBe(4200);
    expect(result.globalAte).toBe(0.014);
    expect(result.heterogeneity.heterogeneityDetected).toBe(true);
  });

  it('passes through already-stripped segment names', async () => {
    // MSW mocks use already-stripped names; real API uses prefixed names.
    // Verify both work.
    server.use(
      http.post(`${ANALYSIS_SVC}/GetCateAnalysis`, () =>
        HttpResponse.json({
          experimentId: 'cate-stripped',
          metricId: 'ctr',
          globalAte: 0.01,
          globalSe: 0.003,
          globalCiLower: 0.004,
          globalCiUpper: 0.016,
          globalPValue: 0.001,
          subgroupEffects: [
            {
              segment: 'TRIAL', // already stripped
              effect: 0.02,
              se: 0.01,
              ciLower: 0.0,
              ciUpper: 0.04,
              pValueRaw: 0.05,
              pValueAdjusted: 0.05,
              isSignificant: true,
              nControl: 1000,
              nTreatment: 1000,
              controlMean: 0.10,
              treatmentMean: 0.12,
            },
          ],
          heterogeneity: {
            qStatistic: 0.5,
            df: 0,
            pValue: 0.48,
            iSquared: 0,
            // heterogeneityDetected: false → omitted
          },
          nSubgroups: 1,
          fdrThreshold: 0.05,
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getCateAnalysis('cate-stripped');
    expect(result.subgroupEffects[0].segment).toBe('TRIAL');
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 6. GST Trajectory — sequential method enum prefix stripping
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: GetGstTrajectory — enum prefix stripping', () => {
  it('strips SEQUENTIAL_METHOD_ prefix from method enum', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetGstTrajectory`, () =>
        HttpResponse.json({
          experimentId: 'gst-1',
          metricId: 'click_through_rate',
          method: 'SEQUENTIAL_METHOD_MSPRT',
          plannedLooks: 5,
          overallAlpha: 0.05,
          boundaryPoints: [
            { look: 1, informationFraction: 0.2, boundaryZScore: 4.56, observedZScore: 1.2 },
            { look: 2, informationFraction: 0.4, boundaryZScore: 3.23, observedZScore: 2.1 },
            { look: 3, informationFraction: 0.6, boundaryZScore: 2.63, observedZScore: 2.8 },
            { look: 4, informationFraction: 0.8, boundaryZScore: 2.28 },
            { look: 5, informationFraction: 1.0, boundaryZScore: 2.04 },
          ],
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getGstTrajectory('gst-1', 'click_through_rate');
    expect(result.method).toBe('MSPRT');
    expect(result.boundaryPoints).toHaveLength(5);
    // Verify future looks have no observedZScore
    expect(result.boundaryPoints[3].observedZScore).toBeUndefined();
  });

  it('strips GST_OBF and GST_POCOCK prefixed methods', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetGstTrajectory`, () =>
        HttpResponse.json({
          experimentId: 'gst-obf',
          metricId: 'metric_a',
          method: 'SEQUENTIAL_METHOD_GST_OBF',
          plannedLooks: 3,
          overallAlpha: 0.05,
          boundaryPoints: [],
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getGstTrajectory('gst-obf', 'metric_a');
    expect(result.method).toBe('GST_OBF');
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 7. Analysis flat response envelope (no wrapper)
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: response envelope patterns', () => {
  it('analysis responses are flat (no { result: {...} } wrapper)', async () => {
    // Unlike management RPCs which wrap in { experiment: {...} },
    // analysis RPCs return the result directly (flat), then adapted.
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'flat-test',
          metricResults: [],
          srmResult: { chiSquared: 0.1, pValue: 0.75 },
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('flat-test');
    // Should parse directly — not try to unwrap from a nested key
    expect(result.experimentId).toBe('flat-test');
    // Adapter applies defaults for missing fields
    expect(result.srmResult.isMismatch).toBe(false);
    expect(result.srmResult.observedCounts).toEqual({});
  });

  it('novelty analysis is flat', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetNoveltyAnalysis`, () =>
        HttpResponse.json({
          experimentId: 'flat-novelty',
          metricId: 'ctr',
          rawTreatmentEffect: 0.01,
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getNoveltyAnalysis('flat-novelty');
    expect(result.experimentId).toBe('flat-novelty');
  });

  it('interference analysis is flat', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetInterferenceAnalysis`, () =>
        HttpResponse.json({
          experimentId: 'flat-interference',
          jensenShannonDivergence: 0.01,
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getInterferenceAnalysis('flat-interference');
    expect(result.experimentId).toBe('flat-interference');
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 8. SurrogateProjection type mismatch (KNOWN GAP)
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: SurrogateProjection contract', () => {
  it('adapts proto SurrogateProjection fields to UI type', async () => {
    // Proto SurrogateProjection has: experiment_id, variant_id, model_id
    // UI SurrogateProjection needs: metricId, surrogateMetricId
    // The adapter maps: modelId → metricId, variantId → surrogateMetricId

    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'surrogate-test',
          metricResults: [],
          srmResult: {},
          surrogateProjections: [
            {
              // Proto-shaped payload (no metricId/surrogateMetricId)
              experimentId: 'surrogate-test',
              variantId: 'v1-treatment',
              modelId: 'model-ltv',
              projectedEffect: 0.008,
              projectionCiLower: 0.002,
              projectionCiUpper: 0.014,
              calibrationRSquared: 0.78,
              computedAt: '2026-03-05T12:00:00Z',
            },
          ],
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('surrogate-test');
    expect(result.surrogateProjections).toHaveLength(1);

    const sp = result.surrogateProjections![0];
    // Numeric fields match both proto and UI type
    expect(sp.projectedEffect).toBe(0.008);
    expect(sp.projectionCiLower).toBe(0.002);
    expect(sp.projectionCiUpper).toBe(0.014);
    expect(sp.calibrationRSquared).toBe(0.78);

    // Adapter maps proto fields → UI fields
    expect(sp.metricId).toBe('model-ltv'); // adapted from modelId
    expect(sp.surrogateMetricId).toBe('v1-treatment'); // adapted from variantId
    // Proto fields preserved for debugging
    expect(sp.modelId).toBe('model-ltv');
    expect(sp.variantId).toBe('v1-treatment');
  });

  it('preserves metricId/surrogateMetricId when already present (MSW mocks)', async () => {
    // MSW mocks use UI-style fields directly. Adapter should prefer them.
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'surrogate-msw',
          metricResults: [],
          srmResult: {},
          surrogateProjections: [
            {
              metricId: 'monthly_retention_rate',
              surrogateMetricId: 'click_through_rate',
              projectedEffect: 0.008,
              projectionCiLower: 0.002,
              projectionCiUpper: 0.014,
              calibrationRSquared: 0.78,
            },
          ],
          computedAt: '2026-03-05T12:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('surrogate-msw');
    const sp = result.surrogateProjections![0];
    // When metricId is already present, adapter keeps it
    expect(sp.metricId).toBe('monthly_retention_rate');
    expect(sp.surrogateMetricId).toBe('click_through_rate');
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 9. IPW-adjusted results (bandit experiment analysis)
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: IPW-adjusted results', () => {
  it('parses MetricResult with nested ipwResult from proto3 JSON', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'ipw-test',
          metricResults: [
            {
              metricId: 'reward_rate',
              variantId: 'arm-b',
              controlMean: 0.312,
              treatmentMean: 0.364,
              absoluteEffect: 0.052,
              relativeEffect: 0.1667,
              ciLower: 0.017,
              ciUpper: 0.087,
              pValue: 0.004,
              isSignificant: true,
              varianceReductionPct: 18,
              cupedAdjustedEffect: 0.048,
              cupedCiLower: 0.019,
              cupedCiUpper: 0.077,
              ipwResult: {
                effect: 0.045,
                se: 0.016,
                ciLower: 0.014,
                ciUpper: 0.076,
                pValue: 0.005,
                nClipped: 15,
                effectiveSampleSize: 4820,
              },
            },
          ],
          srmResult: {
            chiSquared: 0.31,
            pValue: 0.58,
          },
          computedAt: '2026-03-10T08:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('ipw-test');
    expect(result.metricResults).toHaveLength(1);

    const mr = result.metricResults[0];
    expect(mr.ipwResult).toBeDefined();
    expect(mr.ipwResult!.effect).toBe(0.045);
    expect(mr.ipwResult!.se).toBe(0.016);
    expect(mr.ipwResult!.ciLower).toBe(0.014);
    expect(mr.ipwResult!.ciUpper).toBe(0.076);
    expect(mr.ipwResult!.pValue).toBe(0.005);
    expect(mr.ipwResult!.nClipped).toBe(15);
    expect(mr.ipwResult!.effectiveSampleSize).toBe(4820);
  });

  it('handles proto3 zero omission for ipwResult fields', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'ipw-zero',
          metricResults: [
            {
              metricId: 'reward_rate',
              variantId: 'arm-a',
              controlMean: 0.3,
              treatmentMean: 0.3,
              // absoluteEffect: 0.0 → omitted
              // relativeEffect: 0.0 → omitted
              // pValue: 0.0 → omitted
              ipwResult: {
                // effect: 0.0 → omitted by proto3
                se: 0.02,
                // ciLower: 0.0 → omitted
                ciUpper: 0.04,
                // pValue: 0.0 → omitted
                // nClipped: 0 → omitted by proto3
                effectiveSampleSize: 5000,
              },
            },
          ],
          srmResult: {},
          computedAt: '2026-03-10T08:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('ipw-zero');
    const ipw = result.metricResults[0].ipwResult!;
    expect(ipw).toBeDefined();
    expect(ipw.se).toBe(0.02);
    expect(ipw.effectiveSampleSize).toBe(5000);
    // Proto3 zero-omitted fields defaulted to 0 by adaptIpwResult
    expect(ipw.effect).toBe(0);
    expect(ipw.nClipped).toBe(0);
  });

  it('MetricResult without ipwResult (non-bandit experiment)', async () => {
    server.use(
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () =>
        HttpResponse.json({
          experimentId: 'no-ipw',
          metricResults: [
            {
              metricId: 'click_through_rate',
              variantId: 'v1-treatment',
              controlMean: 0.124,
              treatmentMean: 0.138,
              absoluteEffect: 0.014,
              relativeEffect: 0.1129,
              ciLower: 0.003,
              ciUpper: 0.025,
              pValue: 0.008,
              isSignificant: true,
              // no ipwResult field — standard A/B test
            },
          ],
          srmResult: {},
          computedAt: '2026-03-10T08:00:00Z',
        }),
      ),
    );

    const result = await getAnalysisResult('no-ipw');
    expect(result.metricResults[0].ipwResult).toBeUndefined();
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 10. RPCs called by Agent-6 that DON'T exist in proto yet (KNOWN GAPS)
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: RPCs not yet in analysis_service.proto', () => {
  // These RPCs are called by Agent-6 but have no proto definition in
  // proto/experimentation/analysis/v1/analysis_service.proto.
  // They work against MSW mocks. Real API integration requires Agent-4
  // to add these RPCs to the proto.

  it('GetCumulativeHoldoutResult — no proto definition (mock only)', async () => {
    // Expected proto addition:
    //   rpc GetCumulativeHoldoutResult(GetCumulativeHoldoutResultRequest)
    //     returns (CumulativeHoldoutResult);
    const result = await getCumulativeHoldoutResult('77777777-7777-7777-7777-777777777777');
    expect(result.experimentId).toBe('77777777-7777-7777-7777-777777777777');
    expect(result.isSignificant).toBe(true);
    expect(result.timeSeries).toHaveLength(5);
  });

  it('GetGstTrajectory — no proto definition (mock only)', async () => {
    // Expected proto addition:
    //   rpc GetGstTrajectory(GetGstTrajectoryRequest)
    //     returns (GstTrajectoryResult);
    const result = await getGstTrajectory(
      '11111111-1111-1111-1111-111111111111',
      'click_through_rate',
    );
    expect(result.method).toBe('MSPRT');
    expect(result.boundaryPoints.length).toBeGreaterThan(0);
  });

  it('GetQoeDashboard — no proto definition (mock only)', async () => {
    // Expected proto addition:
    //   rpc GetQoeDashboard(GetQoeDashboardRequest) returns (QoeDashboardResult);
    const result = await getQoeDashboard('22222222-2222-2222-2222-222222222222');
    expect(result.experimentId).toBe('22222222-2222-2222-2222-222222222222');
    expect(result.snapshots.length).toBeGreaterThan(0);
  });

  it('GetCateAnalysis — no proto definition (mock only)', async () => {
    // Expected proto addition:
    //   rpc GetCateAnalysis(GetCateAnalysisRequest)
    //     returns (CateAnalysisResult);
    const result = await getCateAnalysis('11111111-1111-1111-1111-111111111111');
    expect(result.experimentId).toBe('11111111-1111-1111-1111-111111111111');
    expect(result.subgroupEffects.length).toBeGreaterThan(0);
  });

  it('GetBanditDashboard — in bandit proto, not analysis (mock only)', async () => {
    // This RPC is routed to BANDIT_SVC, not ANALYSIS_SVC.
    // No proto definition exists in bandit_service.proto.
    // Expected proto addition to bandit/v1/bandit_service.proto:
    //   rpc GetBanditDashboard(GetBanditDashboardRequest)
    //     returns (BanditDashboardResult);
    const result = await getBanditDashboard('44444444-4444-4444-4444-444444444444');
    expect(result.experimentId).toBe('44444444-4444-4444-4444-444444444444');
    expect(result.algorithm).toBe('THOMPSON_SAMPLING');
    expect(result.arms.length).toBeGreaterThan(0);
  });
});

// ────────────────────────────────────────────────────────────────────────────
// 11. UI fields not in proto (KNOWN GAPS — will need proto additions)
// ────────────────────────────────────────────────────────────────────────────

describe('M4a wire format: UI fields absent from proto', () => {
  it('NoveltyAnalysisResult.dailyEffects — not in proto', () => {
    // UI type has dailyEffects: NoveltyDailyEffect[] (day, observedEffect, fittedEffect)
    // Proto NoveltyAnalysisResult has no such field.
    // RECOMMENDATION: Add to proto:
    //   message NoveltyDailyEffect {
    //     int32 day = 1;
    //     double observed_effect = 2;
    //     double fitted_effect = 3;
    //   }
    //   repeated NoveltyDailyEffect daily_effects = 11;
    expect(true).toBe(true); // documentation test
  });

  it('InterferenceAnalysisResult Lorenz curves — not in proto', () => {
    // UI type has treatmentLorenzCurve/controlLorenzCurve: LorenzCurvePoint[]
    // Proto InterferenceAnalysisResult has no Lorenz curve fields.
    // RECOMMENDATION: Add to proto:
    //   message LorenzCurvePoint {
    //     double cumulative_content_fraction = 1;
    //     double cumulative_consumption_fraction = 2;
    //   }
    //   repeated LorenzCurvePoint treatment_lorenz_curve = 11;
    //   repeated LorenzCurvePoint control_lorenz_curve = 12;
    expect(true).toBe(true); // documentation test
  });

  it('AnalysisResult.cochranQPValue — RESOLVED: now in UI type + adapter', () => {
    // Proto has cochran_q_p_value on AnalysisResult (field 5).
    // FIXED: UI AnalysisResult now includes cochranQPValue?: number.
    // Adapter preserves the value from proto JSON.
    expect(true).toBe(true); // documented as resolved
  });

  it('MetricResult.segmentResults — RESOLVED: now in UI type + adapter', () => {
    // Proto MetricResult has repeated SegmentResult segment_results = 16.
    // FIXED: UI MetricResult now includes segmentResults?: SegmentResult[].
    // Adapter strips LIFECYCLE_SEGMENT_ prefix and coerces int64 sampleSize.
    expect(true).toBe(true); // documented as resolved
  });
});
