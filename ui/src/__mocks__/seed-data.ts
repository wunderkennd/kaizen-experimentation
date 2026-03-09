import type {
  AnalysisResult, Experiment, QueryLogEntry,
  NoveltyAnalysisResult, InterferenceAnalysisResult, InterleavingAnalysisResult,
  BanditDashboardResult, CumulativeHoldoutResult, GuardrailStatusResult, QoeDashboardResult,
  GstTrajectoryResult, CateAnalysisResult,
} from '@/lib/types';

const INITIAL_EXPERIMENTS: Experiment[] = [
  {
    experimentId: '11111111-1111-1111-1111-111111111111',
    name: 'homepage_recs_v2',
    description: 'Test new recommendation algorithm on homepage carousel',
    ownerEmail: 'alice@streamco.com',
    type: 'AB',
    state: 'RUNNING',
    variants: [
      {
        variantId: 'v1-control',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"algorithm": "collaborative_filter_v1"}',
      },
      {
        variantId: 'v1-treatment',
        name: 'neural_recs',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"algorithm": "neural_cf_v2", "diversity_boost": 0.3}',
      },
    ],
    layerId: 'layer-homepage',
    hashSalt: 'salt-homepage-recs-v2',
    primaryMetricId: 'click_through_rate',
    secondaryMetricIds: ['watch_time_per_session', 'content_diversity_score'],
    guardrailConfigs: [
      {
        metricId: 'crash_rate',
        threshold: 0.01,
        consecutiveBreachesRequired: 2,
      },
    ],
    guardrailAction: 'AUTO_PAUSE',
    sequentialTestConfig: {
      method: 'MSPRT',
      plannedLooks: 0,
      overallAlpha: 0.05,
    },
    surrogateModelId: 'surrogate-homepage-ltv',
    isCumulativeHoldout: false,
    createdAt: '2026-02-15T10:00:00Z',
    startedAt: '2026-02-16T08:00:00Z',
  },
  {
    experimentId: '22222222-2222-2222-2222-222222222222',
    name: 'adaptive_bitrate_v3',
    description: 'Compare ABR algorithms for playback quality optimization',
    ownerEmail: 'bob@streamco.com',
    type: 'PLAYBACK_QOE',
    state: 'DRAFT',
    variants: [
      {
        variantId: 'v2-control',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"abr": "buffer_based_v2"}',
      },
      {
        variantId: 'v2-treatment',
        name: 'ml_abr',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"abr": "ml_predictive_v1", "lookahead_chunks": 5}',
      },
    ],
    layerId: 'layer-playback',
    hashSalt: 'salt-adaptive-bitrate-v3',
    primaryMetricId: 'rebuffer_ratio',
    secondaryMetricIds: ['time_to_first_frame_ms', 'avg_bitrate_kbps'],
    guardrailConfigs: [
      {
        metricId: 'startup_failure_rate',
        threshold: 0.05,
        consecutiveBreachesRequired: 1,
      },
    ],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2026-03-01T14:30:00Z',
  },
  {
    experimentId: '33333333-3333-3333-3333-333333333333',
    name: 'search_ranking_interleave',
    description: 'Interleave test comparing BM25 vs semantic search ranking',
    ownerEmail: 'carol@streamco.com',
    type: 'INTERLEAVING',
    state: 'RUNNING',
    variants: [
      {
        variantId: 'v3-bm25',
        name: 'bm25_baseline',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"ranker": "bm25", "boost_recency": true}',
      },
      {
        variantId: 'v3-semantic',
        name: 'semantic_search',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"ranker": "semantic_v2", "embedding_model": "e5-large"}',
      },
    ],
    layerId: 'layer-search',
    hashSalt: 'salt-search-ranking-interleave',
    primaryMetricId: 'search_success_rate',
    secondaryMetricIds: ['clicks_per_search', 'time_to_play'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2026-02-20T09:15:00Z',
    startedAt: '2026-02-21T06:00:00Z',
  },
  {
    experimentId: '44444444-4444-4444-4444-444444444444',
    name: 'cold_start_bandit',
    description: 'Contextual bandit for new content cold-start placement',
    ownerEmail: 'dave@streamco.com',
    type: 'CONTEXTUAL_BANDIT',
    state: 'DRAFT',
    variants: [
      {
        variantId: 'v4-arm1',
        name: 'top_carousel',
        trafficFraction: 0.25,
        isControl: true,
        payloadJson: '{"placement": "hero_carousel", "position": 1}',
      },
      {
        variantId: 'v4-arm2',
        name: 'genre_row',
        trafficFraction: 0.25,
        isControl: false,
        payloadJson: '{"placement": "genre_row", "position": 3}',
      },
      {
        variantId: 'v4-arm3',
        name: 'trending_section',
        trafficFraction: 0.25,
        isControl: false,
        payloadJson: '{"placement": "trending", "position": 2}',
      },
      {
        variantId: 'v4-arm4',
        name: 'personalized_row',
        trafficFraction: 0.25,
        isControl: false,
        payloadJson: '{"placement": "for_you", "position": 1}',
      },
    ],
    layerId: 'layer-content-placement',
    hashSalt: 'salt-cold-start-bandit',
    primaryMetricId: 'play_through_rate',
    secondaryMetricIds: ['completion_rate', 'add_to_watchlist_rate'],
    guardrailConfigs: [
      {
        metricId: 'bounce_rate',
        threshold: 0.4,
        consecutiveBreachesRequired: 3,
      },
    ],
    guardrailAction: 'ALERT_ONLY',
    isCumulativeHoldout: false,
    createdAt: '2026-03-02T16:45:00Z',
  },
  // Transitional state experiments for testing StartingChecklist and ConcludingProgress
  {
    experimentId: '55555555-5555-5555-5555-555555555555',
    name: 'onboarding_flow_v2',
    description: 'Testing new onboarding experience for trial users',
    ownerEmail: 'eve@streamco.com',
    type: 'AB',
    state: 'STARTING',
    variants: [
      {
        variantId: 'v5-control',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"flow": "classic"}',
      },
      {
        variantId: 'v5-treatment',
        name: 'guided_onboarding',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"flow": "guided_v2", "steps": 5}',
      },
    ],
    layerId: 'layer-onboarding',
    hashSalt: 'salt-onboarding-flow-v2',
    primaryMetricId: 'trial_conversion_rate',
    secondaryMetricIds: ['profile_completion_rate'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2026-03-03T10:00:00Z',
    startedAt: '2026-03-04T08:00:00Z',
  },
  {
    experimentId: '66666666-6666-6666-6666-666666666666',
    name: 'thumbnail_selection_v1',
    description: 'Concluding analysis of thumbnail optimization experiment',
    ownerEmail: 'frank@streamco.com',
    type: 'AB',
    state: 'CONCLUDING',
    variants: [
      {
        variantId: 'v6-control',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"selector": "editorial"}',
      },
      {
        variantId: 'v6-treatment',
        name: 'ml_thumbnails',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"selector": "ml_attention_v1"}',
      },
    ],
    layerId: 'layer-content',
    hashSalt: 'salt-thumbnail-selection-v1',
    primaryMetricId: 'click_through_rate',
    secondaryMetricIds: ['watch_time_per_session'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2026-02-10T09:00:00Z',
    startedAt: '2026-02-11T06:00:00Z',
  },
  // Cumulative holdout experiment
  {
    experimentId: '77777777-7777-7777-7777-777777777777',
    name: 'recommendation_holdout_q1',
    description: 'Cumulative holdout measuring long-term impact of recommendation algorithm changes',
    ownerEmail: 'alice@streamco.com',
    type: 'CUMULATIVE_HOLDOUT',
    state: 'RUNNING',
    variants: [
      {
        variantId: 'v7-holdout',
        name: 'holdout',
        trafficFraction: 0.05,
        isControl: true,
        payloadJson: '{"algorithm": "collaborative_filter_v1"}',
      },
      {
        variantId: 'v7-treatment',
        name: 'all_changes',
        trafficFraction: 0.95,
        isControl: false,
        payloadJson: '{"algorithm": "latest_production"}',
      },
    ],
    layerId: 'layer-holdout',
    hashSalt: 'salt-recommendation-holdout-q1',
    primaryMetricId: 'monthly_active_days',
    secondaryMetricIds: ['watch_hours_per_month', 'churn_rate'],
    guardrailConfigs: [
      {
        metricId: 'churn_rate',
        threshold: 0.08,
        consecutiveBreachesRequired: 3,
      },
    ],
    guardrailAction: 'ALERT_ONLY',
    isCumulativeHoldout: true,
    createdAt: '2026-01-01T00:00:00Z',
    startedAt: '2026-01-02T00:00:00Z',
  },
];

/** Mock query log entries for RUNNING experiments. */
const INITIAL_QUERY_LOG: Record<string, QueryLogEntry[]> = {
  '11111111-1111-1111-1111-111111111111': [
    {
      experimentId: '11111111-1111-1111-1111-111111111111',
      metricId: 'click_through_rate',
      sqlText: 'SELECT variant_id, COUNT(DISTINCT user_id) AS users, SUM(CASE WHEN clicked THEN 1 ELSE 0 END) AS clicks, SUM(CASE WHEN clicked THEN 1 ELSE 0 END)::FLOAT / COUNT(DISTINCT user_id) AS ctr FROM events.homepage_interactions WHERE experiment_id = \'11111111-1111-1111-1111-111111111111\' AND event_date BETWEEN \'2026-02-16\' AND \'2026-03-05\' GROUP BY variant_id',
      rowCount: 125000,
      durationMs: 3200,
      computedAt: '2026-03-05T14:30:00Z',
    },
    {
      experimentId: '11111111-1111-1111-1111-111111111111',
      metricId: 'watch_time_per_session',
      sqlText: 'SELECT variant_id, AVG(watch_duration_seconds) AS avg_watch_time, STDDEV(watch_duration_seconds) AS stddev_watch_time FROM events.playback_sessions WHERE experiment_id = \'11111111-1111-1111-1111-111111111111\' AND session_date BETWEEN \'2026-02-16\' AND \'2026-03-05\' GROUP BY variant_id',
      rowCount: 98500,
      durationMs: 4100,
      computedAt: '2026-03-05T14:30:05Z',
    },
    {
      experimentId: '11111111-1111-1111-1111-111111111111',
      metricId: 'crash_rate',
      sqlText: 'SELECT variant_id, COUNT(DISTINCT CASE WHEN crashed THEN session_id END)::FLOAT / COUNT(DISTINCT session_id) AS crash_rate FROM events.app_sessions WHERE experiment_id = \'11111111-1111-1111-1111-111111111111\' AND session_date BETWEEN \'2026-02-16\' AND \'2026-03-05\' GROUP BY variant_id',
      rowCount: 250000,
      durationMs: 1800,
      computedAt: '2026-03-05T14:30:10Z',
    },
  ],
  '33333333-3333-3333-3333-333333333333': [
    {
      experimentId: '33333333-3333-3333-3333-333333333333',
      metricId: 'search_success_rate',
      sqlText: 'SELECT variant_id, COUNT(DISTINCT CASE WHEN result_clicked THEN search_id END)::FLOAT / COUNT(DISTINCT search_id) AS success_rate FROM events.search_queries WHERE experiment_id = \'33333333-3333-3333-3333-333333333333\' AND query_date BETWEEN \'2026-02-21\' AND \'2026-03-05\' GROUP BY variant_id',
      rowCount: 75000,
      durationMs: 2400,
      computedAt: '2026-03-05T14:35:00Z',
    },
    {
      experimentId: '33333333-3333-3333-3333-333333333333',
      metricId: 'clicks_per_search',
      sqlText: 'SELECT variant_id, AVG(click_count) AS avg_clicks FROM events.search_queries WHERE experiment_id = \'33333333-3333-3333-3333-333333333333\' AND query_date BETWEEN \'2026-02-21\' AND \'2026-03-05\' GROUP BY variant_id',
      rowCount: 75000,
      durationMs: 450,
      computedAt: '2026-03-05T14:35:02Z',
    },
  ],
};

const INITIAL_ANALYSIS_RESULTS: AnalysisResult[] = [
  {
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
      },
      {
        metricId: 'watch_time_per_session',
        variantId: 'v1-treatment',
        controlMean: 1842,
        treatmentMean: 1956,
        absoluteEffect: 114,
        relativeEffect: 0.0619,
        ciLower: 28,
        ciUpper: 200,
        pValue: 0.009,
        isSignificant: true,
        cupedAdjustedEffect: 108,
        cupedCiLower: 42,
        cupedCiUpper: 174,
        varianceReductionPct: 28,
        sequentialResult: {
          boundaryCrossed: false,
          alphaSpent: 0.019,
          alphaRemaining: 0.031,
          currentLook: 3,
          adjustedPValue: 0.041,
        },
      },
      {
        metricId: 'content_diversity_score',
        variantId: 'v1-treatment',
        controlMean: 0.72,
        treatmentMean: 0.74,
        absoluteEffect: 0.02,
        relativeEffect: 0.0278,
        ciLower: -0.01,
        ciUpper: 0.05,
        pValue: 0.19,
        isSignificant: false,
        cupedAdjustedEffect: 0.018,
        cupedCiLower: -0.005,
        cupedCiUpper: 0.041,
        varianceReductionPct: 18,
        sequentialResult: {
          boundaryCrossed: false,
          alphaSpent: 0.008,
          alphaRemaining: 0.042,
          currentLook: 3,
          adjustedPValue: 0.22,
        },
      },
    ],
    srmResult: {
      chiSquared: 0.42,
      pValue: 0.517,
      isMismatch: false,
      observedCounts: { 'v1-control': 50102, 'v1-treatment': 49898 },
      expectedCounts: { 'v1-control': 50000, 'v1-treatment': 50000 },
    },
    surrogateProjections: [
      {
        metricId: 'monthly_retention_rate',
        surrogateMetricId: 'click_through_rate',
        projectedEffect: 0.008,
        projectionCiLower: 0.002,
        projectionCiUpper: 0.014,
        calibrationRSquared: 0.78,
      },
      {
        metricId: 'lifetime_value',
        surrogateMetricId: 'watch_time_per_session',
        projectedEffect: 2.45,
        projectionCiLower: -0.50,
        projectionCiUpper: 5.40,
        calibrationRSquared: 0.52,
      },
    ],
    computedAt: '2026-03-05T12:00:00Z',
  },
  {
    experimentId: '66666666-6666-6666-6666-666666666666',
    metricResults: [
      {
        metricId: 'click_through_rate',
        variantId: 'v6-treatment',
        controlMean: 0.089,
        treatmentMean: 0.102,
        absoluteEffect: 0.013,
        relativeEffect: 0.1461,
        ciLower: 0.002,
        ciUpper: 0.024,
        pValue: 0.02,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
      },
      {
        metricId: 'watch_time_per_session',
        variantId: 'v6-treatment',
        controlMean: 2100,
        treatmentMean: 2145,
        absoluteEffect: 45,
        relativeEffect: 0.0214,
        ciLower: -30,
        ciUpper: 120,
        pValue: 0.24,
        isSignificant: false,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
      },
    ],
    srmResult: {
      chiSquared: 14.82,
      pValue: 0.0001,
      isMismatch: true,
      observedCounts: { 'v6-control': 52300, 'v6-treatment': 47700 },
      expectedCounts: { 'v6-control': 50000, 'v6-treatment': 50000 },
    },
    computedAt: '2026-03-04T18:30:00Z',
  },
  {
    experimentId: '33333333-3333-3333-3333-333333333333',
    metricResults: [
      {
        metricId: 'search_success_rate',
        variantId: 'v3-semantic',
        controlMean: 0.62,
        treatmentMean: 0.68,
        absoluteEffect: 0.06,
        relativeEffect: 0.0968,
        ciLower: 0.02,
        ciUpper: 0.10,
        pValue: 0.004,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
      },
      {
        metricId: 'clicks_per_search',
        variantId: 'v3-semantic',
        controlMean: 1.8,
        treatmentMean: 2.1,
        absoluteEffect: 0.3,
        relativeEffect: 0.1667,
        ciLower: 0.05,
        ciUpper: 0.55,
        pValue: 0.019,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
      },
    ],
    srmResult: {
      chiSquared: 0.18,
      pValue: 0.671,
      isMismatch: false,
      observedCounts: { 'v3-bm25': 37650, 'v3-semantic': 37350 },
      expectedCounts: { 'v3-bm25': 37500, 'v3-semantic': 37500 },
    },
    computedAt: '2026-03-05T14:35:00Z',
  },
];

/** Novelty analysis mock — homepage_recs_v2 shows novelty decay in CTR. */
const INITIAL_NOVELTY_RESULTS: Record<string, NoveltyAnalysisResult> = {
  '11111111-1111-1111-1111-111111111111': {
    experimentId: '11111111-1111-1111-1111-111111111111',
    metricId: 'click_through_rate',
    noveltyDetected: true,
    rawTreatmentEffect: 0.014,
    projectedSteadyStateEffect: 0.009,
    noveltyAmplitude: 0.018,
    decayConstantDays: 4.2,
    isStabilized: false,
    daysUntilProjectedStability: 6,
    dailyEffects: [
      { day: 1, observedEffect: 0.032, fittedEffect: 0.027 },
      { day: 2, observedEffect: 0.028, fittedEffect: 0.024 },
      { day: 3, observedEffect: 0.022, fittedEffect: 0.021 },
      { day: 4, observedEffect: 0.019, fittedEffect: 0.018 },
      { day: 5, observedEffect: 0.017, fittedEffect: 0.016 },
      { day: 6, observedEffect: 0.016, fittedEffect: 0.014 },
      { day: 7, observedEffect: 0.013, fittedEffect: 0.013 },
      { day: 8, observedEffect: 0.014, fittedEffect: 0.012 },
      { day: 9, observedEffect: 0.011, fittedEffect: 0.011 },
      { day: 10, observedEffect: 0.012, fittedEffect: 0.011 },
      { day: 11, observedEffect: 0.010, fittedEffect: 0.010 },
      { day: 12, observedEffect: 0.011, fittedEffect: 0.010 },
      { day: 13, observedEffect: 0.009, fittedEffect: 0.010 },
      { day: 14, observedEffect: 0.010, fittedEffect: 0.009 },
      { day: 15, observedEffect: 0.009, fittedEffect: 0.009 },
      { day: 16, observedEffect: 0.010, fittedEffect: 0.009 },
      { day: 17, observedEffect: 0.009, fittedEffect: 0.009 },
    ],
    computedAt: '2026-03-05T12:00:00Z',
  },
};

/** Interference analysis mock — homepage_recs_v2 shows content consumption interference. */
const INITIAL_INTERFERENCE_RESULTS: Record<string, InterferenceAnalysisResult> = {
  '11111111-1111-1111-1111-111111111111': {
    experimentId: '11111111-1111-1111-1111-111111111111',
    interferenceDetected: true,
    jensenShannonDivergence: 0.042,
    jaccardSimilarityTop100: 0.73,
    treatmentGiniCoefficient: 0.61,
    controlGiniCoefficient: 0.58,
    treatmentCatalogCoverage: 0.34,
    controlCatalogCoverage: 0.31,
    spilloverTitles: [
      { contentId: 'title-1234', treatmentWatchRate: 0.082, controlWatchRate: 0.041, pValue: 0.002 },
      { contentId: 'title-5678', treatmentWatchRate: 0.015, controlWatchRate: 0.044, pValue: 0.008 },
      { contentId: 'title-9012', treatmentWatchRate: 0.063, controlWatchRate: 0.038, pValue: 0.031 },
    ],
    treatmentLorenzCurve: [
      { cumulativeContentFraction: 0, cumulativeConsumptionFraction: 0 },
      { cumulativeContentFraction: 0.1, cumulativeConsumptionFraction: 0.02 },
      { cumulativeContentFraction: 0.2, cumulativeConsumptionFraction: 0.05 },
      { cumulativeContentFraction: 0.3, cumulativeConsumptionFraction: 0.10 },
      { cumulativeContentFraction: 0.4, cumulativeConsumptionFraction: 0.17 },
      { cumulativeContentFraction: 0.5, cumulativeConsumptionFraction: 0.26 },
      { cumulativeContentFraction: 0.6, cumulativeConsumptionFraction: 0.37 },
      { cumulativeContentFraction: 0.7, cumulativeConsumptionFraction: 0.51 },
      { cumulativeContentFraction: 0.8, cumulativeConsumptionFraction: 0.68 },
      { cumulativeContentFraction: 0.9, cumulativeConsumptionFraction: 0.85 },
      { cumulativeContentFraction: 1.0, cumulativeConsumptionFraction: 1.0 },
    ],
    controlLorenzCurve: [
      { cumulativeContentFraction: 0, cumulativeConsumptionFraction: 0 },
      { cumulativeContentFraction: 0.1, cumulativeConsumptionFraction: 0.03 },
      { cumulativeContentFraction: 0.2, cumulativeConsumptionFraction: 0.07 },
      { cumulativeContentFraction: 0.3, cumulativeConsumptionFraction: 0.13 },
      { cumulativeContentFraction: 0.4, cumulativeConsumptionFraction: 0.20 },
      { cumulativeContentFraction: 0.5, cumulativeConsumptionFraction: 0.29 },
      { cumulativeContentFraction: 0.6, cumulativeConsumptionFraction: 0.40 },
      { cumulativeContentFraction: 0.7, cumulativeConsumptionFraction: 0.54 },
      { cumulativeContentFraction: 0.8, cumulativeConsumptionFraction: 0.70 },
      { cumulativeContentFraction: 0.9, cumulativeConsumptionFraction: 0.86 },
      { cumulativeContentFraction: 1.0, cumulativeConsumptionFraction: 1.0 },
    ],
    computedAt: '2026-03-05T12:00:00Z',
  },
};

/** Interleaving analysis mock — search_ranking_interleave comparing BM25 vs semantic. */
const INITIAL_INTERLEAVING_RESULTS: Record<string, InterleavingAnalysisResult> = {
  '33333333-3333-3333-3333-333333333333': {
    experimentId: '33333333-3333-3333-3333-333333333333',
    algorithmWinRates: { bm25_baseline: 0.42, semantic_search: 0.58 },
    signTestPValue: 0.003,
    algorithmStrengths: [
      { algorithmId: 'bm25_baseline', strength: 0.45, ciLower: 0.39, ciUpper: 0.51 },
      { algorithmId: 'semantic_search', strength: 0.55, ciLower: 0.49, ciUpper: 0.61 },
    ],
    positionAnalyses: [
      { position: 1, algorithmEngagementRates: { bm25_baseline: 0.31, semantic_search: 0.38 } },
      { position: 2, algorithmEngagementRates: { bm25_baseline: 0.24, semantic_search: 0.29 } },
      { position: 3, algorithmEngagementRates: { bm25_baseline: 0.18, semantic_search: 0.23 } },
      { position: 4, algorithmEngagementRates: { bm25_baseline: 0.12, semantic_search: 0.17 } },
      { position: 5, algorithmEngagementRates: { bm25_baseline: 0.09, semantic_search: 0.14 } },
    ],
    computedAt: '2026-03-05T14:35:00Z',
  },
};

/** Bandit dashboard mock — cold_start_bandit with Thompson Sampling. */
const INITIAL_BANDIT_RESULTS: Record<string, BanditDashboardResult> = {
  '44444444-4444-4444-4444-444444444444': {
    experimentId: '44444444-4444-4444-4444-444444444444',
    algorithm: 'THOMPSON_SAMPLING',
    totalRewardsProcessed: 3842,
    snapshotAt: '2026-03-05T16:00:00Z',
    arms: [
      { armId: 'v4-arm1', name: 'top_carousel', selectionCount: 1200, rewardCount: 312, rewardRate: 0.26, assignmentProbability: 0.35, alpha: 313, beta: 889, expectedReward: 0.260 },
      { armId: 'v4-arm2', name: 'genre_row', selectionCount: 980, rewardCount: 196, rewardRate: 0.20, assignmentProbability: 0.18, alpha: 197, beta: 785, expectedReward: 0.201 },
      { armId: 'v4-arm3', name: 'trending_section', selectionCount: 860, rewardCount: 224, rewardRate: 0.26, assignmentProbability: 0.32, alpha: 225, beta: 637, expectedReward: 0.261 },
      { armId: 'v4-arm4', name: 'personalized_row', selectionCount: 802, rewardCount: 144, rewardRate: 0.18, assignmentProbability: 0.15, alpha: 145, beta: 659, expectedReward: 0.180 },
    ],
    isWarmup: false,
    warmupObservations: 1000,
    minExplorationFraction: 0.1,
    rewardHistory: [
      { timestamp: '2026-03-02T00:00:00Z', armId: 'top_carousel', cumulativeReward: 45, cumulativeSelections: 180 },
      { timestamp: '2026-03-02T00:00:00Z', armId: 'genre_row', cumulativeReward: 32, cumulativeSelections: 170 },
      { timestamp: '2026-03-02T00:00:00Z', armId: 'trending_section', cumulativeReward: 38, cumulativeSelections: 160 },
      { timestamp: '2026-03-02T00:00:00Z', armId: 'personalized_row', cumulativeReward: 28, cumulativeSelections: 165 },
      { timestamp: '2026-03-03T00:00:00Z', armId: 'top_carousel', cumulativeReward: 105, cumulativeSelections: 400 },
      { timestamp: '2026-03-03T00:00:00Z', armId: 'genre_row', cumulativeReward: 68, cumulativeSelections: 350 },
      { timestamp: '2026-03-03T00:00:00Z', armId: 'trending_section', cumulativeReward: 82, cumulativeSelections: 340 },
      { timestamp: '2026-03-03T00:00:00Z', armId: 'personalized_row', cumulativeReward: 55, cumulativeSelections: 330 },
      { timestamp: '2026-03-04T00:00:00Z', armId: 'top_carousel', cumulativeReward: 190, cumulativeSelections: 720 },
      { timestamp: '2026-03-04T00:00:00Z', armId: 'genre_row', cumulativeReward: 120, cumulativeSelections: 580 },
      { timestamp: '2026-03-04T00:00:00Z', armId: 'trending_section', cumulativeReward: 155, cumulativeSelections: 560 },
      { timestamp: '2026-03-04T00:00:00Z', armId: 'personalized_row', cumulativeReward: 88, cumulativeSelections: 520 },
      { timestamp: '2026-03-05T00:00:00Z', armId: 'top_carousel', cumulativeReward: 312, cumulativeSelections: 1200 },
      { timestamp: '2026-03-05T00:00:00Z', armId: 'genre_row', cumulativeReward: 196, cumulativeSelections: 980 },
      { timestamp: '2026-03-05T00:00:00Z', armId: 'trending_section', cumulativeReward: 224, cumulativeSelections: 860 },
      { timestamp: '2026-03-05T00:00:00Z', armId: 'personalized_row', cumulativeReward: 144, cumulativeSelections: 802 },
    ],
  },
};

/** Cumulative holdout results — recommendation_holdout_q1 running since Jan 2026. */
const INITIAL_HOLDOUT_RESULTS: Record<string, CumulativeHoldoutResult> = {
  '77777777-7777-7777-7777-777777777777': {
    experimentId: '77777777-7777-7777-7777-777777777777',
    metricId: 'monthly_active_days',
    currentCumulativeLift: 1.8,
    currentCiLower: 0.4,
    currentCiUpper: 3.2,
    isSignificant: true,
    timeSeries: [
      { date: '2026-01-15', cumulativeLift: 0.3, ciLower: -2.1, ciUpper: 2.7, sampleSize: 12000 },
      { date: '2026-01-31', cumulativeLift: 0.8, ciLower: -1.0, ciUpper: 2.6, sampleSize: 24000 },
      { date: '2026-02-15', cumulativeLift: 1.2, ciLower: -0.2, ciUpper: 2.6, sampleSize: 36000 },
      { date: '2026-02-28', cumulativeLift: 1.5, ciLower: 0.1, ciUpper: 2.9, sampleSize: 48000 },
      { date: '2026-03-05', cumulativeLift: 1.8, ciLower: 0.4, ciUpper: 3.2, sampleSize: 55000 },
    ],
    computedAt: '2026-03-05T18:00:00Z',
  },
};

/** Guardrail breach history — homepage_recs_v2 had a brief crash_rate spike. */
const INITIAL_GUARDRAIL_STATUS: Record<string, GuardrailStatusResult> = {
  '11111111-1111-1111-1111-111111111111': {
    experimentId: '11111111-1111-1111-1111-111111111111',
    breaches: [
      {
        experimentId: '11111111-1111-1111-1111-111111111111',
        metricId: 'crash_rate',
        variantId: 'v1-treatment',
        currentValue: 0.012,
        threshold: 0.01,
        consecutiveBreachCount: 1,
        action: 'ALERT',
        detectedAt: '2026-02-28T03:15:00Z',
      },
      {
        experimentId: '11111111-1111-1111-1111-111111111111',
        metricId: 'crash_rate',
        variantId: 'v1-treatment',
        currentValue: 0.014,
        threshold: 0.01,
        consecutiveBreachCount: 2,
        action: 'AUTO_PAUSE',
        detectedAt: '2026-02-28T06:30:00Z',
      },
    ],
    isPaused: false,
  },
};

/** QoE dashboard mock — adaptive_bitrate_v3 with playback quality metrics. */
const INITIAL_QOE_RESULTS: Record<string, QoeDashboardResult> = {
  '22222222-2222-2222-2222-222222222222': {
    experimentId: '22222222-2222-2222-2222-222222222222',
    snapshots: [
      {
        metricId: 'time_to_first_frame_ms',
        label: 'Time to First Frame',
        controlValue: 1200,
        treatmentValue: 950,
        unit: 'ms',
        lowerIsBetter: true,
        warningThreshold: 1500,
        criticalThreshold: 2500,
        status: 'GOOD',
      },
      {
        metricId: 'rebuffer_ratio',
        label: 'Rebuffer Ratio',
        controlValue: 0.015,
        treatmentValue: 0.008,
        unit: '%',
        lowerIsBetter: true,
        warningThreshold: 0.02,
        criticalThreshold: 0.05,
        status: 'GOOD',
      },
      {
        metricId: 'avg_bitrate_kbps',
        label: 'Average Bitrate',
        controlValue: 4200,
        treatmentValue: 5100,
        unit: 'kbps',
        lowerIsBetter: false,
        warningThreshold: 3000,
        criticalThreshold: 1500,
        status: 'GOOD',
      },
      {
        metricId: 'resolution_switches',
        label: 'Resolution Switches',
        controlValue: 2.3,
        treatmentValue: 1.1,
        unit: '/session',
        lowerIsBetter: true,
        warningThreshold: 3.0,
        criticalThreshold: 5.0,
        status: 'GOOD',
      },
      {
        metricId: 'startup_failure_rate',
        label: 'Startup Failure Rate',
        controlValue: 0.020,
        treatmentValue: 0.035,
        unit: '%',
        lowerIsBetter: true,
        warningThreshold: 0.03,
        criticalThreshold: 0.05,
        status: 'WARNING',
      },
    ],
    overallStatus: 'WARNING',
    computedAt: '2026-03-05T16:00:00Z',
  },
};

/** GST boundary trajectory mock — homepage_recs_v2 using MSPRT with 5 planned looks. */
const INITIAL_GST_RESULTS: Record<string, GstTrajectoryResult[]> = {
  '11111111-1111-1111-1111-111111111111': [
    {
      experimentId: '11111111-1111-1111-1111-111111111111',
      metricId: 'click_through_rate',
      method: 'MSPRT',
      plannedLooks: 5,
      overallAlpha: 0.05,
      boundaryPoints: [
        { look: 1, informationFraction: 0.20, boundaryZScore: 4.56, observedZScore: 1.2 },
        { look: 2, informationFraction: 0.40, boundaryZScore: 3.23, observedZScore: 2.1 },
        { look: 3, informationFraction: 0.60, boundaryZScore: 2.63, observedZScore: 2.8 },
        { look: 4, informationFraction: 0.80, boundaryZScore: 2.28 },
        { look: 5, informationFraction: 1.00, boundaryZScore: 2.04 },
      ],
      computedAt: '2026-03-05T12:00:00Z',
    },
  ],
};

/** CATE lifecycle segment analysis — homepage_recs_v2 shows heterogeneous effects. */
const INITIAL_CATE_RESULTS: Record<string, CateAnalysisResult> = {
  '11111111-1111-1111-1111-111111111111': {
    experimentId: '11111111-1111-1111-1111-111111111111',
    metricId: 'click_through_rate',
    globalAte: 0.014,
    globalSe: 0.0042,
    globalCiLower: 0.006,
    globalCiUpper: 0.022,
    globalPValue: 0.001,
    subgroupEffects: [
      {
        segment: 'TRIAL',
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
        segment: 'NEW',
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
        segment: 'ESTABLISHED',
        effect: 0.008,
        se: 0.005,
        ciLower: -0.002,
        ciUpper: 0.018,
        pValueRaw: 0.11,
        pValueAdjusted: 0.165,
        isSignificant: false,
        nControl: 18200,
        nTreatment: 18100,
        controlMean: 0.129,
        treatmentMean: 0.137,
      },
      {
        segment: 'MATURE',
        effect: 0.004,
        se: 0.006,
        ciLower: -0.008,
        ciUpper: 0.016,
        pValueRaw: 0.51,
        pValueAdjusted: 0.51,
        isSignificant: false,
        nControl: 15800,
        nTreatment: 15900,
        controlMean: 0.131,
        treatmentMean: 0.135,
      },
    ],
    heterogeneity: {
      qStatistic: 12.4,
      df: 3,
      pValue: 0.006,
      iSquared: 75.8,
      heterogeneityDetected: true,
    },
    nSubgroups: 4,
    fdrThreshold: 0.05,
    computedAt: '2026-03-05T12:00:00Z',
  },
};

/** Mutable copy of seed data — MSW handlers mutate this in-place. */
export let SEED_EXPERIMENTS: Experiment[] = structuredClone(INITIAL_EXPERIMENTS);
export let SEED_QUERY_LOG: Record<string, QueryLogEntry[]> = structuredClone(INITIAL_QUERY_LOG);
export let SEED_ANALYSIS_RESULTS: AnalysisResult[] = structuredClone(INITIAL_ANALYSIS_RESULTS);
export let SEED_NOVELTY_RESULTS: Record<string, NoveltyAnalysisResult> = structuredClone(INITIAL_NOVELTY_RESULTS);
export let SEED_INTERFERENCE_RESULTS: Record<string, InterferenceAnalysisResult> = structuredClone(INITIAL_INTERFERENCE_RESULTS);
export let SEED_INTERLEAVING_RESULTS: Record<string, InterleavingAnalysisResult> = structuredClone(INITIAL_INTERLEAVING_RESULTS);
export let SEED_BANDIT_RESULTS: Record<string, BanditDashboardResult> = structuredClone(INITIAL_BANDIT_RESULTS);
export let SEED_HOLDOUT_RESULTS: Record<string, CumulativeHoldoutResult> = structuredClone(INITIAL_HOLDOUT_RESULTS);
export let SEED_GUARDRAIL_STATUS: Record<string, GuardrailStatusResult> = structuredClone(INITIAL_GUARDRAIL_STATUS);
export let SEED_QOE_RESULTS: Record<string, QoeDashboardResult> = structuredClone(INITIAL_QOE_RESULTS);
export let SEED_GST_RESULTS: Record<string, GstTrajectoryResult[]> = structuredClone(INITIAL_GST_RESULTS);
export let SEED_CATE_RESULTS: Record<string, CateAnalysisResult> = structuredClone(INITIAL_CATE_RESULTS);

/** Reset seed data to initial state. Call in afterEach for test isolation. */
export function resetSeedData(): void {
  SEED_EXPERIMENTS = structuredClone(INITIAL_EXPERIMENTS);
  SEED_QUERY_LOG = structuredClone(INITIAL_QUERY_LOG);
  SEED_ANALYSIS_RESULTS = structuredClone(INITIAL_ANALYSIS_RESULTS);
  SEED_NOVELTY_RESULTS = structuredClone(INITIAL_NOVELTY_RESULTS);
  SEED_INTERFERENCE_RESULTS = structuredClone(INITIAL_INTERFERENCE_RESULTS);
  SEED_INTERLEAVING_RESULTS = structuredClone(INITIAL_INTERLEAVING_RESULTS);
  SEED_BANDIT_RESULTS = structuredClone(INITIAL_BANDIT_RESULTS);
  SEED_HOLDOUT_RESULTS = structuredClone(INITIAL_HOLDOUT_RESULTS);
  SEED_GUARDRAIL_STATUS = structuredClone(INITIAL_GUARDRAIL_STATUS);
  SEED_QOE_RESULTS = structuredClone(INITIAL_QOE_RESULTS);
  SEED_GST_RESULTS = structuredClone(INITIAL_GST_RESULTS);
  SEED_CATE_RESULTS = structuredClone(INITIAL_CATE_RESULTS);
}
