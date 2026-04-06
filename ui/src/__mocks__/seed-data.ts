import type {
  AnalysisResult, Experiment, QueryLogEntry,
  NoveltyAnalysisResult, InterferenceAnalysisResult, InterleavingAnalysisResult,
  BanditDashboardResult, CumulativeHoldoutResult, GuardrailStatusResult, QoeDashboardResult,
  GstTrajectoryResult, CateAnalysisResult, Layer, LayerAllocation, MetricDefinition,
  AuditLogEntry, Flag, ProviderHealthResult,
  AvlmResult, AdaptiveNResult, FeedbackLoopResult, OnlineFdrState, OptimalAlphaRecommendation,
  PortfolioAllocationResult, PortfolioMetricsResult, ParetoFrontierResult, MetaExperimentResult,
  SlateOpeResult, SlateHeatmapResult,
  SwitchbackResult, SyntheticControlResult,
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
    onlineFdrConfig: {
      targetAlpha: 0.05,
      initialWealth: 0.05,
      strategy: 'E_LOND',
    },
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
    qoeConfig: {
      qoeMetrics: ['rebuffer_ratio', 'time_to_first_frame_ms', 'avg_bitrate_kbps', 'startup_failure_rate'],
      deviceFilter: 'smart_tv',
    },
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
    interleavingConfig: {
      method: 'TEAM_DRAFT',
      algorithmIds: ['bm25_v2', 'semantic_search_v2'],
      creditAssignment: 'BINARY_WIN',
      creditMetricEvent: 'click',
      maxListSize: 20,
    },
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
    banditExperimentConfig: {
      algorithm: 'THOMPSON_SAMPLING',
      rewardMetricId: 'play_through_rate',
      contextFeatureKeys: ['content_genre', 'user_tenure_days', 'device_type'],
      minExplorationFraction: 0.1,
      warmupObservations: 200,
      rewardObjectives: [
        { metricId: 'play_through_rate', weight: 0.6, floor: 0.0, isPrimary: true },
        { metricId: 'provider_diversity_score', weight: 0.25, floor: 0.3, isPrimary: false },
        { metricId: 'add_to_watchlist_rate', weight: 0.15, floor: 0.0, isPrimary: false },
      ],
      compositionMethod: 'WEIGHTED_SCALARIZATION',
      globalConstraints: [
        { label: 'max_single_provider_share', coefficients: { 'v4-arm1': 1.0, 'v4-arm2': 0.0, 'v4-arm3': 0.0, 'v4-arm4': 0.0 }, rhs: 0.5 },
        { label: 'min_diversity_floor', coefficients: { 'v4-arm1': -0.25, 'v4-arm2': -0.25, 'v4-arm3': -0.25, 'v4-arm4': -0.25 }, rhs: -0.2 },
      ],
    },
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
  // CONCLUDED experiment for testing results links
  {
    experimentId: '88888888-8888-8888-8888-888888888888',
    name: 'retention_nudge_v1',
    description: 'A/B test of push notification nudge for retention improvement',
    ownerEmail: 'alice@streamco.com',
    type: 'AB',
    state: 'CONCLUDED',
    variants: [
      {
        variantId: 'v8-control',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"nudge": "none"}',
      },
      {
        variantId: 'v8-treatment',
        name: 'smart_nudge',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"nudge": "ml_timed_v1", "max_per_day": 2}',
      },
    ],
    layerId: 'layer-engagement',
    hashSalt: 'salt-retention-nudge-v1',
    primaryMetricId: 'day_7_retention',
    secondaryMetricIds: ['notification_open_rate', 'unsubscribe_rate'],
    guardrailConfigs: [
      {
        metricId: 'unsubscribe_rate',
        threshold: 0.03,
        consecutiveBreachesRequired: 2,
      },
    ],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2026-01-20T11:00:00Z',
    startedAt: '2026-01-21T06:00:00Z',
    concludedAt: '2026-02-20T18:00:00Z',
  },
  // SESSION_LEVEL experiment for testing session-level analysis
  {
    experimentId: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    name: 'session_watch_pattern',
    description: 'Session-level test of watch pattern recommendations with HC1 clustered SE',
    ownerEmail: 'carol@streamco.com',
    type: 'SESSION_LEVEL',
    state: 'CONCLUDED',
    variants: [
      {
        variantId: 'va-control',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"recommendation": "standard"}',
      },
      {
        variantId: 'va-treatment',
        name: 'session_aware',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"recommendation": "session_context_v2", "lookback_sessions": 3}',
      },
    ],
    layerId: 'layer-engagement',
    hashSalt: 'salt-session-watch-pattern',
    primaryMetricId: 'watch_time_per_session',
    secondaryMetricIds: ['sessions_per_week'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2026-02-01T09:00:00Z',
    startedAt: '2026-02-02T06:00:00Z',
    concludedAt: '2026-03-02T18:00:00Z',
  },
  // SWITCHBACK experiment (ADR-022)
  {
    experimentId: 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee',
    name: 'delivery_speed_switchback_v1',
    description: 'Switchback experiment measuring delivery speed treatment on rider earnings',
    ownerEmail: 'grace@streamco.com',
    type: 'SWITCHBACK',
    state: 'CONCLUDED',
    variants: [
      {
        variantId: 'vc-control',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{"speed_tier": "standard"}',
      },
      {
        variantId: 'vc-treatment',
        name: 'priority_delivery',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{"speed_tier": "priority_v2"}',
      },
    ],
    layerId: 'layer-homepage',
    hashSalt: 'salt-delivery-speed-switchback-v1',
    primaryMetricId: 'click_through_rate',
    secondaryMetricIds: ['watch_time_per_session'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2026-01-10T09:00:00Z',
    startedAt: '2026-01-11T00:00:00Z',
    concludedAt: '2026-02-08T00:00:00Z',
  },
  // QUASI_EXPERIMENT — synthetic control (ADR-023)
  {
    experimentId: 'dddddddd-dddd-dddd-dddd-dddddddddddd',
    name: 'market_expansion_synthetic_control',
    description: 'Synthetic control analysis for regional market expansion launch',
    ownerEmail: 'henry@streamco.com',
    type: 'QUASI_EXPERIMENT',
    state: 'CONCLUDED',
    variants: [
      {
        variantId: 'vd-treated',
        name: 'treated_market',
        trafficFraction: 1.0,
        isControl: false,
        payloadJson: '{"region": "pacific_northwest"}',
      },
    ],
    layerId: 'layer-homepage',
    hashSalt: 'salt-market-expansion-synthetic-control',
    primaryMetricId: 'click_through_rate',
    secondaryMetricIds: ['watch_time_per_session'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2025-10-01T00:00:00Z',
    startedAt: '2025-10-15T00:00:00Z',
    concludedAt: '2026-01-15T00:00:00Z',
  },
  // ARCHIVED experiment for testing archived state
  {
    experimentId: '99999999-9999-9999-9999-999999999999',
    name: 'legacy_layout_test',
    description: 'Multivariate test of legacy browse page layout options',
    ownerEmail: 'bob@streamco.com',
    type: 'MULTIVARIATE',
    state: 'ARCHIVED',
    variants: [
      {
        variantId: 'v9-control',
        name: 'control',
        trafficFraction: 0.34,
        isControl: true,
        payloadJson: '{"layout": "grid_3col"}',
      },
      {
        variantId: 'v9-variant-a',
        name: 'list_layout',
        trafficFraction: 0.33,
        isControl: false,
        payloadJson: '{"layout": "list_vertical"}',
      },
      {
        variantId: 'v9-variant-b',
        name: 'carousel_layout',
        trafficFraction: 0.33,
        isControl: false,
        payloadJson: '{"layout": "horizontal_carousel"}',
      },
    ],
    layerId: 'layer-browse',
    hashSalt: 'salt-legacy-layout-test',
    primaryMetricId: 'browse_to_play_rate',
    secondaryMetricIds: ['time_on_page', 'scroll_depth'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    createdAt: '2025-11-15T09:00:00Z',
    startedAt: '2025-11-16T06:00:00Z',
    concludedAt: '2025-12-20T18:00:00Z',
  },
  // Slate bandit experiment (ADR-016)
  {
    experimentId: 'cccccccc-cccc-cccc-cccc-cccccccccccc',
    name: 'homepage_slate_v1',
    description: 'Slot-wise factorized Thompson Sampling for homepage recommendation slate',
    ownerEmail: 'grace@streamco.com',
    type: 'SLATE',
    state: 'RUNNING',
    variants: [],
    layerId: 'layer-homepage',
    hashSalt: 'salt-homepage-slate-v1',
    primaryMetricId: 'click_through_rate',
    secondaryMetricIds: ['watch_time_per_session', 'completion_rate'],
    guardrailConfigs: [
      {
        metricId: 'bounce_rate',
        threshold: 0.45,
        consecutiveBreachesRequired: 2,
      },
    ],
    guardrailAction: 'ALERT_ONLY',
    isCumulativeHoldout: false,
    banditExperimentConfig: {
      algorithm: 'THOMPSON_SAMPLING',
      rewardMetricId: 'click_through_rate',
      contextFeatureKeys: ['user_tenure_days', 'device_type', 'content_genre'],
      minExplorationFraction: 0.05,
      warmupObservations: 10000,
    },
    createdAt: '2026-03-20T08:00:00Z',
    startedAt: '2026-03-21T06:00:00Z',
  },
  // META experiment (ADR-013)
  {
    experimentId: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    name: 'meta_bandit_comparison',
    description: 'Meta-experiment comparing Thompson Sampling vs Linear UCB for homepage recommendations',
    ownerEmail: 'irene@streamco.com',
    type: 'META',
    state: 'RUNNING',
    variants: [
      {
        variantId: 'v-ctrl',
        name: 'control',
        trafficFraction: 0.5,
        isControl: true,
        payloadJson: '{}',
      },
      {
        variantId: 'v-treat',
        name: 'treatment',
        trafficFraction: 0.5,
        isControl: false,
        payloadJson: '{}',
      },
    ],
    layerId: 'layer-homepage',
    hashSalt: 'salt-meta-bandit-comparison',
    primaryMetricId: 'click_through_rate',
    secondaryMetricIds: ['watch_time_per_session'],
    guardrailConfigs: [],
    guardrailAction: 'AUTO_PAUSE',
    isCumulativeHoldout: false,
    metaConfig: {
      variantBanditConfigs: [
        { variantId: 'v-ctrl', banditType: 'THOMPSON_SAMPLING', arms: ['arm-a', 'arm-b'] },
        { variantId: 'v-treat', banditType: 'LINEAR_UCB', arms: ['arm-x', 'arm-y', 'arm-z'] },
      ],
    },
    createdAt: '2026-03-18T08:00:00Z',
    startedAt: '2026-03-19T06:00:00Z',
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
    // ADR-018: e-value alongside p-value for primary metric comparison
    eValueResult: {
      eValue: 12.5,
      logEValue: Math.log(12.5),
      impliedLevel: 1 / 12.5,
      reject: false,  // needs eValue >= 1/0.05 = 20 to reject at α=0.05
      alpha: 0.05,
    },
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
  {
    experimentId: '88888888-8888-8888-8888-888888888888',
    metricResults: [
      {
        metricId: 'day_7_retention',
        variantId: 'v8-treatment',
        controlMean: 0.42,
        treatmentMean: 0.47,
        absoluteEffect: 0.05,
        relativeEffect: 0.119,
        ciLower: 0.02,
        ciUpper: 0.08,
        pValue: 0.001,
        isSignificant: true,
        cupedAdjustedEffect: 0.048,
        cupedCiLower: 0.025,
        cupedCiUpper: 0.071,
        varianceReductionPct: 35,
      },
      {
        metricId: 'notification_open_rate',
        variantId: 'v8-treatment',
        controlMean: 0.15,
        treatmentMean: 0.22,
        absoluteEffect: 0.07,
        relativeEffect: 0.467,
        ciLower: 0.04,
        ciUpper: 0.10,
        pValue: 0.0001,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
      },
    ],
    srmResult: {
      chiSquared: 0.28,
      pValue: 0.597,
      isMismatch: false,
      observedCounts: { 'v8-control': 30150, 'v8-treatment': 29850 },
      expectedCounts: { 'v8-control': 30000, 'v8-treatment': 30000 },
    },
    computedAt: '2026-02-20T18:00:00Z',
  },
  {
    experimentId: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
    metricResults: [
      {
        metricId: 'watch_time_per_session',
        variantId: 'va-treatment',
        controlMean: 1920,
        treatmentMean: 2064,
        absoluteEffect: 144,
        relativeEffect: 0.075,
        ciLower: 42,
        ciUpper: 246,
        pValue: 0.006,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
        sessionLevelResult: {
          naiveSe: 48.2,
          clusteredSe: 69.8,
          designEffect: 2.1,
          naivePValue: 0.003,
          clusteredPValue: 0.039,
        },
      },
      {
        metricId: 'sessions_per_week',
        variantId: 'va-treatment',
        controlMean: 4.2,
        treatmentMean: 4.5,
        absoluteEffect: 0.3,
        relativeEffect: 0.0714,
        ciLower: -0.05,
        ciUpper: 0.65,
        pValue: 0.093,
        isSignificant: false,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
        sessionLevelResult: {
          naiveSe: 0.15,
          clusteredSe: 0.171,
          designEffect: 1.3,
          naivePValue: 0.046,
          clusteredPValue: 0.079,
        },
      },
    ],
    srmResult: {
      chiSquared: 0.31,
      pValue: 0.578,
      isMismatch: false,
      observedCounts: { 'va-control': 20150, 'va-treatment': 19850 },
      expectedCounts: { 'va-control': 20000, 'va-treatment': 20000 },
    },
    computedAt: '2026-03-02T18:00:00Z',
  },
  // delivery_speed_switchback_v1 — minimal analysis result to allow results page to render
  {
    experimentId: 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee',
    metricResults: [
      {
        metricId: 'click_through_rate',
        variantId: 'vc-treatment',
        controlMean: 0.118,
        treatmentMean: 0.131,
        absoluteEffect: 0.013,
        relativeEffect: 0.110,
        ciLower: 0.002,
        ciUpper: 0.024,
        pValue: 0.022,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
      },
    ],
    srmResult: {
      chiSquared: 0.12,
      pValue: 0.73,
      isMismatch: false,
      observedCounts: { 'vc-control': 5120, 'vc-treatment': 5080 },
      expectedCounts: { 'vc-control': 5100, 'vc-treatment': 5100 },
    },
    computedAt: '2026-02-08T06:00:00Z',
  },
  // market_expansion_synthetic_control — minimal analysis result
  {
    experimentId: 'dddddddd-dddd-dddd-dddd-dddddddddddd',
    metricResults: [
      {
        metricId: 'click_through_rate',
        variantId: 'vd-treated',
        controlMean: 0.105,
        treatmentMean: 0.119,
        absoluteEffect: 0.014,
        relativeEffect: 0.133,
        ciLower: 0.005,
        ciUpper: 0.023,
        pValue: 0.003,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
      },
    ],
    srmResult: {
      chiSquared: 0.0,
      pValue: 1.0,
      isMismatch: false,
      observedCounts: { 'vd-treated': 1 },
      expectedCounts: { 'vd-treated': 1 },
    },
    computedAt: '2026-01-15T06:00:00Z',
  },
  // cold_start_bandit — CONTEXTUAL_BANDIT with IPW-adjusted results
  {
    experimentId: '44444444-4444-4444-4444-444444444444',
    metricResults: [
      {
        metricId: 'play_through_rate',
        variantId: 'v4-arm2',
        controlMean: 0.42,
        treatmentMean: 0.48,
        absoluteEffect: 0.06,
        relativeEffect: 0.1429,
        ciLower: 0.01,
        ciUpper: 0.11,
        pValue: 0.019,
        isSignificant: true,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
        ipwResult: {
          effect: 0.052,
          se: 0.018,
          ciLower: 0.017,
          ciUpper: 0.087,
          pValue: 0.004,
          isSignificant: true,
          nClipped: 23,
          effectiveSampleSize: 4820,
        },
      },
      {
        metricId: 'completion_rate',
        variantId: 'v4-arm2',
        controlMean: 0.35,
        treatmentMean: 0.38,
        absoluteEffect: 0.03,
        relativeEffect: 0.0857,
        ciLower: -0.02,
        ciUpper: 0.08,
        pValue: 0.21,
        isSignificant: false,
        cupedAdjustedEffect: 0,
        cupedCiLower: 0,
        cupedCiUpper: 0,
        varianceReductionPct: 0,
        ipwResult: {
          effect: 0.027,
          se: 0.022,
          ciLower: -0.016,
          ciUpper: 0.070,
          pValue: 0.22,
          isSignificant: false,
          nClipped: 23,
          effectiveSampleSize: 4820,
        },
      },
    ],
    srmResult: {
      chiSquared: 1.8,
      pValue: 0.41,
      isMismatch: false,
      observedCounts: { 'v4-arm1': 1250, 'v4-arm2': 1310, 'v4-arm3': 1220, 'v4-arm4': 1240 },
      expectedCounts: { 'v4-arm1': 1255, 'v4-arm2': 1255, 'v4-arm3': 1255, 'v4-arm4': 1255 },
    },
    computedAt: '2026-03-10T14:00:00Z',
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
    objectiveBreakdowns: [
      {
        armId: 'v4-arm1', armName: 'top_carousel',
        objectiveContributions: { play_through_rate: 0.156, provider_diversity_score: 0.112, add_to_watchlist_rate: 0.052 },
        composedReward: 0.320,
      },
      {
        armId: 'v4-arm2', armName: 'genre_row',
        objectiveContributions: { play_through_rate: 0.120, provider_diversity_score: 0.178, add_to_watchlist_rate: 0.039 },
        composedReward: 0.337,
      },
      {
        armId: 'v4-arm3', armName: 'trending_section',
        objectiveContributions: { play_through_rate: 0.149, provider_diversity_score: 0.095, add_to_watchlist_rate: 0.048 },
        composedReward: 0.292,
      },
      {
        armId: 'v4-arm4', armName: 'personalized_row',
        objectiveContributions: { play_through_rate: 0.108, provider_diversity_score: 0.140, add_to_watchlist_rate: 0.031 },
        composedReward: 0.279,
      },
    ],
    constraintStatuses: [
      { label: 'max_single_provider_share', currentValue: 0.3500, limit: 0.5000, isSatisfied: true },
      { label: 'min_diversity_floor', currentValue: -0.2550, limit: -0.2000, isSatisfied: false },
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

/** Layer definitions for bucket allocation visualization. */
const INITIAL_LAYERS: Record<string, Layer> = {
  'layer-homepage': {
    layerId: 'layer-homepage',
    name: 'Homepage',
    description: 'Homepage recommendations',
    totalBuckets: 10000,
  },
  'layer-playback': {
    layerId: 'layer-playback',
    name: 'Playback',
    description: 'Playback QoE experiments',
    totalBuckets: 10000,
  },
  'layer-search': {
    layerId: 'layer-search',
    name: 'Search',
    description: 'Search ranking experiments',
    totalBuckets: 10000,
  },
  'layer-content-placement': {
    layerId: 'layer-content-placement',
    name: 'Content Placement',
    description: 'Content placement optimization',
    totalBuckets: 10000,
  },
  'layer-onboarding': {
    layerId: 'layer-onboarding',
    name: 'Onboarding',
    description: 'User onboarding flows',
    totalBuckets: 10000,
  },
  'layer-content': {
    layerId: 'layer-content',
    name: 'Content',
    description: 'Content presentation experiments',
    totalBuckets: 10000,
  },
  'layer-holdout': {
    layerId: 'layer-holdout',
    name: 'Holdout',
    description: 'Cumulative holdout experiments',
    totalBuckets: 10000,
  },
  'layer-engagement': {
    layerId: 'layer-engagement',
    name: 'Engagement',
    description: 'User engagement experiments',
    totalBuckets: 10000,
  },
  'layer-browse': {
    layerId: 'layer-browse',
    name: 'Browse',
    description: 'Browse page layout experiments',
    totalBuckets: 10000,
  },
};

/** Layer allocations showing which experiments own which bucket ranges. */
const INITIAL_LAYER_ALLOCATIONS: Record<string, LayerAllocation[]> = {
  'layer-homepage': [
    {
      allocationId: 'alloc-1',
      layerId: 'layer-homepage',
      experimentId: '11111111-1111-1111-1111-111111111111',
      startBucket: 0,
      endBucket: 4999,
      activatedAt: '2026-02-16T08:00:00Z',
    },
  ],
  'layer-search': [
    {
      allocationId: 'alloc-2',
      layerId: 'layer-search',
      experimentId: '33333333-3333-3333-3333-333333333333',
      startBucket: 0,
      endBucket: 4999,
      activatedAt: '2026-02-21T06:00:00Z',
    },
    {
      allocationId: 'alloc-3',
      layerId: 'layer-search',
      experimentId: 'archived-search-exp',
      startBucket: 5000,
      endBucket: 7499,
      activatedAt: '2026-01-10T00:00:00Z',
      releasedAt: '2026-02-01T00:00:00Z',
    },
  ],
  'layer-playback': [],
  'layer-content-placement': [],
  'layer-onboarding': [
    {
      allocationId: 'alloc-4',
      layerId: 'layer-onboarding',
      experimentId: '55555555-5555-5555-5555-555555555555',
      startBucket: 0,
      endBucket: 9999,
      activatedAt: '2026-03-04T08:00:00Z',
    },
  ],
  'layer-content': [
    {
      allocationId: 'alloc-5',
      layerId: 'layer-content',
      experimentId: '66666666-6666-6666-6666-666666666666',
      startBucket: 0,
      endBucket: 4999,
      activatedAt: '2026-02-11T06:00:00Z',
    },
  ],
  'layer-holdout': [
    {
      allocationId: 'alloc-6',
      layerId: 'layer-holdout',
      experimentId: '77777777-7777-7777-7777-777777777777',
      startBucket: 0,
      endBucket: 9999,
      activatedAt: '2026-01-02T00:00:00Z',
    },
  ],
  'layer-engagement': [
    {
      allocationId: 'alloc-7',
      layerId: 'layer-engagement',
      experimentId: '88888888-8888-8888-8888-888888888888',
      startBucket: 0,
      endBucket: 5999,
      activatedAt: '2026-01-21T06:00:00Z',
    },
  ],
  'layer-browse': [
    {
      allocationId: 'alloc-8',
      layerId: 'layer-browse',
      experimentId: '99999999-9999-9999-9999-999999999999',
      startBucket: 0,
      endBucket: 9999,
      activatedAt: '2025-11-16T06:00:00Z',
      releasedAt: '2025-12-20T18:00:00Z',
    },
  ],
};

// ---------------------------------------------------------------------------
// Metric Definitions — 10 from SQL seed + 2 edge-case extras (COUNT, CUSTOM)
// ---------------------------------------------------------------------------
const INITIAL_METRIC_DEFINITIONS: MetricDefinition[] = [
  {
    metricId: 'stream_start_rate',
    name: 'Stream Start Rate',
    description: 'Proportion of sessions that start a stream',
    type: 'PROPORTION',
    sourceEventType: 'stream_start',
    lowerIsBetter: false,
    isQoeMetric: false,
  },
  {
    metricId: 'watch_time_minutes',
    name: 'Watch Time (minutes)',
    description: 'Average minutes watched per user per day',
    type: 'MEAN',
    sourceEventType: 'heartbeat',
    lowerIsBetter: false,
    isQoeMetric: false,
    cupedCovariateMetricId: 'watch_time_minutes_pre',
    minimumDetectableEffect: 0.02,
  },
  {
    metricId: 'completion_rate',
    name: 'Content Completion Rate',
    description: 'Proportion of content items watched to ≥90%',
    type: 'PROPORTION',
    sourceEventType: 'stream_end',
    lowerIsBetter: false,
    isQoeMetric: false,
  },
  {
    metricId: 'rebuffer_rate',
    name: 'Rebuffer Rate',
    description: 'Rebuffer events per hour of playback',
    type: 'RATIO',
    sourceEventType: 'qoe_rebuffer',
    numeratorEventType: 'qoe_rebuffer',
    denominatorEventType: 'playback_hour',
    lowerIsBetter: true,
    isQoeMetric: true,
  },
  {
    metricId: 'search_success_rate',
    name: 'Search Success Rate',
    description: 'Proportion of searches that lead to a stream start within 5m',
    type: 'PROPORTION',
    sourceEventType: 'search',
    lowerIsBetter: false,
    isQoeMetric: false,
  },
  {
    metricId: 'ctr_recommendation',
    name: 'Recommendation CTR',
    description: 'Click-through rate on recommendation carousels',
    type: 'PROPORTION',
    sourceEventType: 'impression',
    lowerIsBetter: false,
    isQoeMetric: false,
    surrogateTargetMetricId: 'watch_time_minutes',
  },
  {
    metricId: 'revenue_per_user',
    name: 'Revenue per User',
    description: 'Average daily revenue per user (ads + subscription)',
    type: 'MEAN',
    sourceEventType: 'revenue',
    lowerIsBetter: false,
    isQoeMetric: false,
    minimumDetectableEffect: 0.05,
  },
  {
    metricId: 'churn_7d',
    name: 'Churn (7-day)',
    description: 'Proportion of users with zero activity in trailing 7 days',
    type: 'PROPORTION',
    sourceEventType: 'session',
    lowerIsBetter: true,
    isQoeMetric: false,
  },
  {
    metricId: 'latency_p50_ms',
    name: 'Playback Start Latency p50',
    description: 'Median time-to-first-frame in milliseconds',
    type: 'PERCENTILE',
    sourceEventType: 'playback_start',
    percentile: 50,
    lowerIsBetter: true,
    isQoeMetric: true,
  },
  {
    metricId: 'error_rate',
    name: 'Error Rate',
    description: 'Proportion of playback attempts that result in an error',
    type: 'PROPORTION',
    sourceEventType: 'playback_error',
    lowerIsBetter: true,
    isQoeMetric: true,
  },
  // Edge-case extras: COUNT and CUSTOM types
  {
    metricId: 'daily_active_users',
    name: 'Daily Active Users',
    description: 'Count of unique users with at least one session per day',
    type: 'COUNT',
    sourceEventType: 'session',
    lowerIsBetter: false,
    isQoeMetric: false,
  },
  {
    metricId: 'engagement_score',
    name: 'Engagement Score',
    description: 'Custom composite engagement metric combining watch time, interactions, and session frequency',
    type: 'CUSTOM',
    sourceEventType: 'composite',
    customSql: 'SELECT user_id, (0.5 * watch_minutes + 0.3 * interactions + 0.2 * sessions) AS score FROM user_daily_agg',
    lowerIsBetter: false,
    isQoeMetric: false,
  },
];

// --- Audit Log seed data ---

const INITIAL_AUDIT_LOG: AuditLogEntry[] = [
  {
    entryId: 'audit-001',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'CREATED',
    actorEmail: 'alice@streamco.com',
    timestamp: '2026-01-15T09:00:00Z',
    details: 'Created experiment homepage_recs_v2 (A/B test)',
  },
  {
    entryId: 'audit-002',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'CONFIG_CHANGED',
    actorEmail: 'alice@streamco.com',
    timestamp: '2026-01-16T10:30:00Z',
    details: 'Updated variant traffic allocation',
    previousValue: '{"control": 0.5, "neural_recs": 0.5}',
    newValue: '{"control": 0.6, "neural_recs": 0.4}',
  },
  {
    entryId: 'audit-003',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'STARTED',
    actorEmail: 'alice@streamco.com',
    timestamp: '2026-01-20T14:00:00Z',
    details: 'Experiment moved from DRAFT to RUNNING',
  },
  {
    entryId: 'audit-004',
    experimentId: '22222222-2222-2222-2222-222222222222',
    experimentName: 'search_ranking_boost',
    action: 'CREATED',
    actorEmail: 'bob@streamco.com',
    timestamp: '2026-01-22T08:15:00Z',
    details: 'Created experiment search_ranking_boost (A/B test)',
  },
  {
    entryId: 'audit-005',
    experimentId: '22222222-2222-2222-2222-222222222222',
    experimentName: 'search_ranking_boost',
    action: 'STARTED',
    actorEmail: 'bob@streamco.com',
    timestamp: '2026-01-25T11:00:00Z',
    details: 'Experiment moved from DRAFT to RUNNING',
  },
  {
    entryId: 'audit-006',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'GUARDRAIL_BREACH',
    actorEmail: 'system@streamco.com',
    timestamp: '2026-02-01T03:45:00Z',
    details: 'crash_rate exceeded threshold (0.012 > 0.01) for variant neural_recs',
    previousValue: '0.008',
    newValue: '0.012',
  },
  {
    entryId: 'audit-007',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'PAUSED',
    actorEmail: 'system@streamco.com',
    timestamp: '2026-02-01T03:45:01Z',
    details: 'Auto-paused due to guardrail breach on crash_rate',
  },
  {
    entryId: 'audit-008',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'RESUMED',
    actorEmail: 'alice@streamco.com',
    timestamp: '2026-02-02T10:00:00Z',
    details: 'Manually resumed after crash_rate stabilized',
  },
  {
    entryId: 'audit-009',
    experimentId: '33333333-3333-3333-3333-333333333333',
    experimentName: 'playback_buffer_strategy',
    action: 'CREATED',
    actorEmail: 'carol@streamco.com',
    timestamp: '2026-02-05T09:30:00Z',
    details: 'Created experiment playback_buffer_strategy (Playback QoE)',
  },
  {
    entryId: 'audit-010',
    experimentId: '33333333-3333-3333-3333-333333333333',
    experimentName: 'playback_buffer_strategy',
    action: 'UPDATED',
    actorEmail: 'carol@streamco.com',
    timestamp: '2026-02-06T14:20:00Z',
    details: 'Updated experiment description and secondary metrics',
    previousValue: '{"secondaryMetricIds": ["rebuffer_rate"]}',
    newValue: '{"secondaryMetricIds": ["rebuffer_rate", "startup_time"]}',
  },
  {
    entryId: 'audit-011',
    experimentId: '22222222-2222-2222-2222-222222222222',
    experimentName: 'search_ranking_boost',
    action: 'CONCLUDED',
    actorEmail: 'bob@streamco.com',
    timestamp: '2026-02-15T16:00:00Z',
    details: 'Experiment moved from RUNNING to CONCLUDED — statistical significance reached',
  },
  {
    entryId: 'audit-012',
    experimentId: '22222222-2222-2222-2222-222222222222',
    experimentName: 'search_ranking_boost',
    action: 'ARCHIVED',
    actorEmail: 'admin@streamco.com',
    timestamp: '2026-02-20T09:00:00Z',
    details: 'Experiment archived after winner rolled out',
  },
  {
    entryId: 'audit-013',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'CONFIG_CHANGED',
    actorEmail: 'alice@streamco.com',
    timestamp: '2026-02-25T11:30:00Z',
    details: 'Reverted variant traffic allocation to equal split',
    previousValue: '{"control": 0.6, "neural_recs": 0.4}',
    newValue: '{"control": 0.5, "neural_recs": 0.5}',
  },
  {
    entryId: 'audit-014',
    experimentId: '11111111-1111-1111-1111-111111111111',
    experimentName: 'homepage_recs_v2',
    action: 'CONCLUDED',
    actorEmail: 'alice@streamco.com',
    timestamp: '2026-03-01T15:00:00Z',
    details: 'Experiment moved from RUNNING to CONCLUDED',
  },
  {
    entryId: 'audit-015',
    experimentId: '33333333-3333-3333-3333-333333333333',
    experimentName: 'playback_buffer_strategy',
    action: 'STARTED',
    actorEmail: 'carol@streamco.com',
    timestamp: '2026-03-05T08:00:00Z',
    details: 'Experiment moved from DRAFT to RUNNING',
  },
];

const INITIAL_FLAGS: Flag[] = [
  {
    flagId: 'flag-bool-rollout',
    name: 'dark_mode_rollout',
    description: 'Progressive dark mode rollout to subscribers',
    type: 'BOOLEAN',
    defaultValue: 'false',
    enabled: true,
    rolloutPercentage: 0.5,
    variants: [],
    targetingRuleId: 'rule-premium-users',
  },
  {
    flagId: 'flag-string-ab',
    name: 'checkout_flow_variant',
    description: 'A/B test for checkout page layout',
    type: 'STRING',
    defaultValue: 'control',
    enabled: true,
    rolloutPercentage: 1.0,
    variants: [
      { variantId: 'v-ctrl', value: 'control', trafficFraction: 0.5 },
      { variantId: 'v-new', value: 'streamlined', trafficFraction: 0.5 },
    ],
  },
  {
    flagId: 'flag-disabled-zero',
    name: 'upcoming_feature',
    description: 'Not yet launched — proto3 zero-value test',
    type: 'BOOLEAN',
    defaultValue: 'false',
    enabled: false,
    rolloutPercentage: 0,
    variants: [],
  },
  {
    flagId: 'flag-json-config',
    name: 'player_config_override',
    description: 'JSON config override for video player settings',
    type: 'JSON',
    defaultValue: '{"bitrate":"auto"}',
    enabled: true,
    rolloutPercentage: 0.25,
    variants: [
      { variantId: 'v-low', value: '{"bitrate":"720p","buffer":2}', trafficFraction: 0.34 },
      { variantId: 'v-mid', value: '{"bitrate":"1080p","buffer":4}', trafficFraction: 0.33 },
      { variantId: 'v-high', value: '{"bitrate":"4k","buffer":8}', trafficFraction: 0.33 },
    ],
  },
];

/** Mutable copy of seed data — MSW handlers mutate this in-place. */
export let SEED_FLAGS: Flag[] = structuredClone(INITIAL_FLAGS);
export let SEED_METRIC_DEFINITIONS: MetricDefinition[] = structuredClone(INITIAL_METRIC_DEFINITIONS);
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
export let SEED_LAYERS: Record<string, Layer> = structuredClone(INITIAL_LAYERS);
export let SEED_LAYER_ALLOCATIONS: Record<string, LayerAllocation[]> = structuredClone(INITIAL_LAYER_ALLOCATIONS);
export let SEED_AUDIT_LOG: AuditLogEntry[] = structuredClone(INITIAL_AUDIT_LOG);

// --- Provider Health Seed Data (ADR-014) ---

function makeProviderPoints(
  baseCoverage: number,
  baseGini: number,
  baseLongTail: number,
  trendCoverage: number,
  trendGini: number,
  trendLongTail: number,
): ProviderHealthResult['series'][number]['points'] {
  // 14 daily data points starting 2026-03-09
  return Array.from({ length: 14 }, (_, i) => {
    const date = new Date('2026-03-09');
    date.setDate(date.getDate() + i);
    const noise = (k: number) => Math.sin(i * k) * 0.005;
    return {
      date: date.toISOString().slice(0, 10),
      catalogCoverage: Math.min(1, Math.max(0, baseCoverage + trendCoverage * i + noise(0.8))),
      providerGini: Math.min(1, Math.max(0, baseGini + trendGini * i + noise(1.1))),
      longTailImpressionShare: Math.min(1, Math.max(0, baseLongTail + trendLongTail * i + noise(0.6))),
    };
  });
}

const INITIAL_PROVIDER_HEALTH: ProviderHealthResult = {
  providers: [
    { providerId: 'prov-originals', providerName: 'StreamCo Originals' },
    { providerId: 'prov-studio-a', providerName: 'Studio A' },
    { providerId: 'prov-indie', providerName: 'Indie Collective' },
  ],
  series: [
    {
      providerId: 'prov-originals',
      providerName: 'StreamCo Originals',
      experimentId: '11111111-1111-1111-1111-111111111111',
      experimentName: 'homepage_recs_v2',
      points: makeProviderPoints(0.68, 0.42, 0.14, 0.003, -0.002, 0.004),
    },
    {
      providerId: 'prov-originals',
      providerName: 'StreamCo Originals',
      experimentId: '33333333-3333-3333-3333-333333333333',
      experimentName: 'search_ranking_interleave',
      points: makeProviderPoints(0.71, 0.40, 0.16, 0.002, -0.001, 0.003),
    },
    {
      providerId: 'prov-studio-a',
      providerName: 'Studio A',
      experimentId: '11111111-1111-1111-1111-111111111111',
      experimentName: 'homepage_recs_v2',
      points: makeProviderPoints(0.55, 0.51, 0.11, 0.004, -0.003, 0.005),
    },
    {
      providerId: 'prov-indie',
      providerName: 'Indie Collective',
      experimentId: '11111111-1111-1111-1111-111111111111',
      experimentName: 'homepage_recs_v2',
      points: makeProviderPoints(0.38, 0.62, 0.22, 0.005, -0.004, 0.003),
    },
  ],
  computedAt: '2026-03-23T00:00:00Z',
};

export let SEED_PROVIDER_HEALTH: ProviderHealthResult = structuredClone(INITIAL_PROVIDER_HEALTH);

// --- AVLM Seed Data (ADR-015) ---

function makeAvlmPoints(nLooks: number, finalEstimate: number, reductionPct: number): AvlmResult['boundaryPoints'] {
  return Array.from({ length: nLooks }, (_, i) => {
    const frac = (i + 1) / nLooks;
    const shrink = Math.sqrt(frac); // boundaries shrink as information accumulates
    const est = finalEstimate * shrink;
    const raw = est * (1 + (1 - reductionPct / 100) * 0.2);
    return {
      look: i + 1,
      informationFraction: frac,
      upperBound: est + (0.05 / shrink) * 1.2,
      lowerBound: est - (0.05 / shrink) * 1.2,
      estimate: est,
      estimateRaw: raw,
    };
  });
}

const INITIAL_AVLM_RESULTS: AvlmResult[] = [
  {
    experimentId: '11111111-1111-1111-1111-111111111111',
    metricId: 'click_through_rate',
    boundaryPoints: makeAvlmPoints(5, 0.014, 32),
    varianceReductionPct: 32,
    isConclusive: true,
    conclusiveLook: 3,
    finalEstimate: 0.014,
    finalCiLower: 0.005,
    finalCiUpper: 0.023,
    computedAt: '2026-03-05T14:30:00Z',
  },
  {
    experimentId: '11111111-1111-1111-1111-111111111111',
    metricId: 'watch_time_per_session',
    boundaryPoints: makeAvlmPoints(5, 108, 28),
    varianceReductionPct: 28,
    isConclusive: false,
    finalEstimate: 108,
    finalCiLower: 42,
    finalCiUpper: 174,
    computedAt: '2026-03-05T14:30:05Z',
  },
];

export let SEED_AVLM_RESULTS: AvlmResult[] = structuredClone(INITIAL_AVLM_RESULTS);

// --- Adaptive N Seed Data (ADR-020) ---

function makeTimelinePoints(startDate: string, days: number, plannedN: number): AdaptiveNResult['timelineProjection'] {
  const start = new Date(startDate);
  return Array.from({ length: days }, (_, i) => ({
    date: new Date(start.getTime() + i * 86400_000).toISOString().slice(0, 10),
    estimatedN: Math.round(plannedN * ((i + 1) / days)),
  }));
}

const INITIAL_ADAPTIVE_N_RESULTS: AdaptiveNResult[] = [
  {
    experimentId: '11111111-1111-1111-1111-111111111111',
    zone: 'FAVORABLE',
    currentN: 95000,
    plannedN: 120000,
    conditionalPower: 0.94,
    projectedConclusionDate: '2026-03-20T00:00:00Z',
    timelineProjection: makeTimelinePoints('2026-02-16', 14, 120000),
    computedAt: '2026-03-05T14:30:00Z',
  },
  {
    experimentId: '88888888-8888-8888-8888-888888888888',
    zone: 'PROMISING',
    currentN: 80000,
    plannedN: 120000,
    recommendedN: 150000,
    conditionalPower: 0.67,
    projectedConclusionDate: '2026-04-15T00:00:00Z',
    extensionDays: 21,
    timelineProjection: makeTimelinePoints('2026-01-21', 30, 150000),
    computedAt: '2026-02-15T12:00:00Z',
  },
];

export let SEED_ADAPTIVE_N_RESULTS: AdaptiveNResult[] = structuredClone(INITIAL_ADAPTIVE_N_RESULTS);

// --- Feedback Loop Seed Data ---

const INITIAL_FEEDBACK_LOOP_RESULTS: FeedbackLoopResult[] = [
  {
    experimentId: '11111111-1111-1111-1111-111111111111',
    retrainingEvents: [
      {
        eventId: 're-001',
        retrainedAt: '2026-02-22T02:00:00Z',
        triggerReason: 'Scheduled weekly retrain',
        modelVersion: 'neural_cf_v2.1',
      },
      {
        eventId: 're-002',
        retrainedAt: '2026-03-01T02:00:00Z',
        triggerReason: 'Scheduled weekly retrain',
        modelVersion: 'neural_cf_v2.2',
      },
    ],
    prePostComparison: [
      { date: '2026-02-20', preEffect: 0.011, postEffect: 0.014 },
      { date: '2026-02-21', preEffect: 0.012, postEffect: 0.015 },
      { date: '2026-02-22', preEffect: 0.013, postEffect: 0.014 },
      { date: '2026-02-23', preEffect: 0.014, postEffect: 0.013 },
      { date: '2026-02-24', preEffect: 0.013, postEffect: 0.014 },
      { date: '2026-03-01', preEffect: 0.014, postEffect: 0.016 },
      { date: '2026-03-02', preEffect: 0.015, postEffect: 0.015 },
      { date: '2026-03-03', preEffect: 0.014, postEffect: 0.014 },
    ],
    contaminationTimeline: [
      { date: '2026-02-22', contaminationFraction: 0.0 },
      { date: '2026-02-23', contaminationFraction: 0.08 },
      { date: '2026-02-24', contaminationFraction: 0.15 },
      { date: '2026-02-25', contaminationFraction: 0.19 },
      { date: '2026-02-26', contaminationFraction: 0.21 },
      { date: '2026-03-01', contaminationFraction: 0.24 },
      { date: '2026-03-02', contaminationFraction: 0.22 },
      { date: '2026-03-03', contaminationFraction: 0.20 },
    ],
    rawEstimate: 0.014,
    biasCorrectedEstimate: 0.013,
    biasCorrectedCiLower: 0.004,
    biasCorrectedCiUpper: 0.022,
    contaminationFraction: 0.20,
    recommendations: [
      {
        recommendationId: 'rec-001',
        severity: 'HIGH',
        title: 'Enable pre-period washout',
        description: 'Exclude 48h of data after each retraining event to allow the model to stabilize.',
        action: 'Apply washout window in metric computation SQL.',
      },
      {
        recommendationId: 'rec-002',
        severity: 'MEDIUM',
        title: 'Use bias-corrected estimate for decision',
        description: 'The raw estimate inflates treatment effect due to feedback contamination. Use the doubly-robust corrected estimate.',
        action: 'Override primary decision with biasCorrectedEstimate.',
      },
      {
        recommendationId: 'rec-003',
        severity: 'LOW',
        title: 'Reduce retraining frequency during experiment',
        description: 'Retraining more than once per week creates persistent feedback bias. Consider pausing retrains or using a holdout model.',
        action: 'Coordinate with ML platform team to freeze model during active experiment windows.',
      },
    ],
    computedAt: '2026-03-05T14:30:00Z',
  },
];

export let SEED_FEEDBACK_LOOP_RESULTS: FeedbackLoopResult[] = structuredClone(INITIAL_FEEDBACK_LOOP_RESULTS);

// --- Online FDR State (ADR-018) ---

const INITIAL_ONLINE_FDR_STATES: OnlineFdrState[] = [
  {
    experimentId: '11111111-1111-1111-1111-111111111111',
    alphaWealth: 0.032,
    initialWealth: 0.05,
    numTested: 15,
    numRejected: 3,
    currentFdr: 0.04,
    computedAt: '2026-03-24T10:00:00Z',
  },
];

export let SEED_ONLINE_FDR_STATES: OnlineFdrState[] = structuredClone(INITIAL_ONLINE_FDR_STATES);

// --- Optimal Alpha Recommendation (ADR-019) ---

const INITIAL_OPTIMAL_ALPHA: OptimalAlphaRecommendation[] = [
  {
    optimalAlpha: 0.10,
    expectedPortfolioFdr: 0.042,
    computedAt: '2026-03-24T10:00:00Z',
  },
];

export let SEED_OPTIMAL_ALPHA: OptimalAlphaRecommendation[] = structuredClone(INITIAL_OPTIMAL_ALPHA);

// --- Portfolio Optimization (ADR-019) ---

const INITIAL_PORTFOLIO_ALLOCATION: PortfolioAllocationResult = {
  experiments: [
    {
      experimentId: '11111111-1111-1111-1111-111111111111',
      name: 'homepage_recs_v2',
      effectSize: 0.0312,
      variance: 0.000487,
      allocatedTrafficPct: 0.20,
      priorityScore: 0.872,
      userSegments: ['new', 'established'],
    },
    {
      experimentId: '22222222-2222-2222-2222-222222222222',
      name: 'search_ranking_v3',
      effectSize: 0.0218,
      variance: 0.000312,
      allocatedTrafficPct: 0.15,
      priorityScore: 0.741,
      userSegments: ['established', 'mature'],
    },
    {
      experimentId: '33333333-3333-3333-3333-333333333333',
      name: 'playback_buffer_opt',
      effectSize: -0.0045,
      variance: 0.000198,
      allocatedTrafficPct: 0.10,
      priorityScore: 0.534,
      userSegments: ['trial', 'new'],
    },
    {
      experimentId: '44444444-4444-4444-4444-444444444444',
      name: 'content_diversity_boost',
      effectSize: 0.0089,
      variance: 0.000654,
      allocatedTrafficPct: 0.12,
      priorityScore: 0.421,
      userSegments: ['mature', 'at_risk'],
    },
  ],
  totalAllocatedPct: 0.57,
  computedAt: '2026-03-24T10:00:00Z',
};

export let SEED_PORTFOLIO_ALLOCATION: PortfolioAllocationResult = structuredClone(INITIAL_PORTFOLIO_ALLOCATION);

// --- Slate OPE Seed Data (ADR-016) ---

const INITIAL_SLATE_OPE_RESULTS: SlateOpeResult[] = [
  {
    experimentId: 'cccccccc-cccc-cccc-cccc-cccccccccccc',
    positionBias: [
      { position: 1, ctr: 0.28, lipsWeight: 1.0 },
      { position: 2, ctr: 0.19, lipsWeight: 0.82 },
      { position: 3, ctr: 0.14, lipsWeight: 0.68 },
      { position: 4, ctr: 0.10, lipsWeight: 0.57 },
      { position: 5, ctr: 0.07, lipsWeight: 0.48 },
      { position: 6, ctr: 0.05, lipsWeight: 0.40 },
      { position: 7, ctr: 0.04, lipsWeight: 0.34 },
      { position: 8, ctr: 0.03, lipsWeight: 0.29 },
      { position: 9, ctr: 0.02, lipsWeight: 0.25 },
      { position: 10, ctr: 0.02, lipsWeight: 0.21 },
    ],
    estimatedValue: 0.1423,
    computedAt: '2026-03-23T12:00:00Z',
  },
];

export let SEED_SLATE_OPE_RESULTS: SlateOpeResult[] = structuredClone(INITIAL_SLATE_OPE_RESULTS);

// --- Switchback Seed Data (ADR-022) ---

function makeSwitchbackBlocks(): SwitchbackResult['blocks'] {
  const blocks = [];
  const base = new Date('2026-01-11T00:00:00Z');
  const treatments: Array<'TREATMENT' | 'CONTROL'> = [
    'CONTROL', 'TREATMENT', 'CONTROL', 'TREATMENT', 'CONTROL',
    'TREATMENT', 'TREATMENT', 'CONTROL', 'TREATMENT', 'CONTROL',
    'CONTROL', 'TREATMENT',
  ];
  const outcomes = [0.115, 0.132, 0.118, 0.129, 0.113, 0.134, 0.131, 0.117, 0.133, 0.114, 0.116, 0.130];
  for (let i = 0; i < 12; i++) {
    const start = new Date(base.getTime() + i * 3 * 86400_000);
    const end = new Date(base.getTime() + (i + 1) * 3 * 86400_000);
    blocks.push({
      blockId: `blk-${String(i + 1).padStart(2, '0')}`,
      periodStart: start.toISOString(),
      periodEnd: end.toISOString(),
      treatment: treatments[i],
      outcome: outcomes[i],
      n: 800 + Math.round(Math.sin(i) * 50),
    });
  }
  return blocks;
}

const INITIAL_SWITCHBACK_RESULTS: SwitchbackResult[] = [
  {
    experimentId: 'eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee',
    metricId: 'click_through_rate',
    blocks: makeSwitchbackBlocks(),
    ate: 0.0155,
    ateSe: 0.0048,
    ateCiLower: 0.0061,
    ateCiUpper: 0.0249,
    riPValue: 0.031,
    riNullDistribution: Array.from({ length: 500 }, () => Math.random()),
    acfPoints: [
      { lag: 1, acf: 0.08,  ciLower: -0.28, ciUpper: 0.28 },
      { lag: 2, acf: -0.12, ciLower: -0.28, ciUpper: 0.28 },
      { lag: 3, acf: 0.04,  ciLower: -0.28, ciUpper: 0.28 },
      { lag: 4, acf: -0.06, ciLower: -0.28, ciUpper: 0.28 },
      { lag: 5, acf: 0.02,  ciLower: -0.28, ciUpper: 0.28 },
    ],
    carryoverDetected: false,
    nTreatmentBlocks: 6,
    nControlBlocks: 6,
    computedAt: '2026-02-08T06:00:00Z',
  },
];

export let SEED_SWITCHBACK_RESULTS: SwitchbackResult[] = structuredClone(INITIAL_SWITCHBACK_RESULTS);

// --- Synthetic Control Seed Data (ADR-023) ---

function makeSyntheticControlTimeSeries(): SyntheticControlResult['timeSeries'] {
  const points = [];
  const base = new Date('2025-10-01');
  // 105 daily points; treatment starts at index 14 (2025-10-15)
  for (let i = 0; i < 105; i++) {
    const d = new Date(base.getTime() + i * 86400_000);
    const date = d.toISOString().slice(0, 10);
    const noise = (Math.sin(i * 0.4) * 0.003);
    const treated = i < 14
      ? 0.105 + noise
      : 0.119 + noise + (i - 14) * 0.0001; // gradual ramp after treatment
    const synthetic = 0.105 + noise * 0.7;
    points.push({
      date,
      treated: +treated.toFixed(4),
      synthetic: +synthetic.toFixed(4),
      ciLower: +(synthetic - 0.008).toFixed(4),
      ciUpper: +(synthetic + 0.008).toFixed(4),
    });
  }
  return points;
}

function makeSyntheticControlEffects(): SyntheticControlResult['effects'] {
  const effects = [];
  const base = new Date('2025-10-15'); // treatment start
  let cumulative = 0;
  for (let i = 0; i < 91; i++) {
    const d = new Date(base.getTime() + i * 86400_000);
    const pointwise = 0.014 + Math.sin(i * 0.3) * 0.002;
    cumulative += pointwise;
    effects.push({
      date: d.toISOString().slice(0, 10),
      pointwiseEffect: +pointwise.toFixed(4),
      cumulativeEffect: +cumulative.toFixed(4),
    });
  }
  return effects;
}

function makePlaceboSeries(offset: number): Array<{ date: string; effect: number }> {
  return Array.from({ length: 91 }, (_, i) => {
    const d = new Date(new Date('2025-10-15').getTime() + i * 86400_000);
    return {
      date: d.toISOString().slice(0, 10),
      effect: +(Math.sin(i * 0.3 + offset) * 0.004 + offset * 0.001).toFixed(4),
    };
  });
}

const INITIAL_SYNTHETIC_CONTROL_RESULTS: SyntheticControlResult[] = [
  {
    experimentId: 'dddddddd-dddd-dddd-dddd-dddddddddddd',
    metricId: 'click_through_rate',
    treatmentStartDate: '2025-10-15T00:00:00Z',
    timeSeries: makeSyntheticControlTimeSeries(),
    effects: makeSyntheticControlEffects(),
    donorWeights: [
      { donorId: 'donor-seattle', donorName: 'Seattle Metro', weight: 0.42 },
      { donorId: 'donor-portland', donorName: 'Portland', weight: 0.31 },
      { donorId: 'donor-denver', donorName: 'Denver', weight: 0.18 },
      { donorId: 'donor-boise', donorName: 'Boise', weight: 0.07 },
      { donorId: 'donor-spokane', donorName: 'Spokane', weight: 0.02 },
    ],
    placeboResults: [
      {
        donorId: 'donor-seattle',
        donorName: 'Seattle Metro',
        preRmspe: 0.0031,
        postRmspe: 0.0045,
        rmspeRatio: 1.45,
        series: makePlaceboSeries(0.5),
      },
      {
        donorId: 'donor-portland',
        donorName: 'Portland',
        preRmspe: 0.0041,
        postRmspe: 0.0068,
        rmspeRatio: 1.66,
        series: makePlaceboSeries(-0.3),
      },
      {
        donorId: 'donor-denver',
        donorName: 'Denver',
        preRmspe: 0.0055,
        postRmspe: 0.0102,
        rmspeRatio: 1.85,
        series: makePlaceboSeries(0.9),
      },
      {
        donorId: 'donor-boise',
        donorName: 'Boise',
        preRmspe: 0.0048,
        postRmspe: 0.0079,
        rmspeRatio: 1.65,
        series: makePlaceboSeries(-0.7),
      },
      {
        donorId: 'donor-spokane',
        donorName: 'Spokane',
        preRmspe: 0.0062,
        postRmspe: 0.0139,
        rmspeRatio: 2.24,
        series: makePlaceboSeries(0.2),
      },
    ],
    preRmspe: 0.0029,
    postRmspe: 0.0142,
    rmspeRatio: 4.90,
    pValue: 0.033,
    isSignificant: true,
    computedAt: '2026-01-15T06:00:00Z',
  },
];

export let SEED_SYNTHETIC_CONTROL_RESULTS: SyntheticControlResult[] = structuredClone(INITIAL_SYNTHETIC_CONTROL_RESULTS);

// --- Portfolio Metrics (ADR-019 extension) ---

const DATES_7D = ['2026-03-18', '2026-03-19', '2026-03-20', '2026-03-21', '2026-03-22', '2026-03-23', '2026-03-24'];

const INITIAL_PORTFOLIO_METRICS: PortfolioMetricsResult = {
  winRates: [
    ...DATES_7D.flatMap((date, i) => [
      { date, experimentId: '11111111-1111-1111-1111-111111111111', experimentName: 'homepage_recs_v2', winRate: 0.52 + i * 0.01 },
      { date, experimentId: '22222222-2222-2222-2222-222222222222', experimentName: 'search_ranking_v3', winRate: 0.48 + i * 0.015 },
      { date, experimentId: '33333333-3333-3333-3333-333333333333', experimentName: 'playback_buffer_opt', winRate: 0.45 + i * 0.005 },
    ]),
  ],
  learningRates: [
    ...DATES_7D.flatMap((date, i) => [
      { date, experimentId: '11111111-1111-1111-1111-111111111111', experimentName: 'homepage_recs_v2', learningRate: 0.12 + i * 0.02, samplesProcessed: 5000 + i * 1000 },
      { date, experimentId: '22222222-2222-2222-2222-222222222222', experimentName: 'search_ranking_v3', learningRate: 0.08 + i * 0.025, samplesProcessed: 3000 + i * 800 },
    ]),
  ],
  annualizedImpacts: [
    { experimentId: '11111111-1111-1111-1111-111111111111', experimentName: 'homepage_recs_v2', annualizedImpact: 0.0312, ciLower: 0.0112, ciUpper: 0.0512, metricId: 'engagement_rate' },
    { experimentId: '22222222-2222-2222-2222-222222222222', experimentName: 'search_ranking_v3', annualizedImpact: 0.0218, ciLower: 0.0018, ciUpper: 0.0418, metricId: 'click_through_rate' },
    { experimentId: '33333333-3333-3333-3333-333333333333', experimentName: 'playback_buffer_opt', annualizedImpact: -0.0045, ciLower: -0.0145, ciUpper: 0.0055, metricId: 'rebuffer_rate' },
    { experimentId: '44444444-4444-4444-4444-444444444444', experimentName: 'content_diversity_boost', annualizedImpact: 0.0089, ciLower: -0.0011, ciUpper: 0.0189, metricId: 'catalog_coverage' },
  ],
  computedAt: '2026-03-24T10:00:00Z',
};

export let SEED_PORTFOLIO_METRICS: PortfolioMetricsResult = structuredClone(INITIAL_PORTFOLIO_METRICS);

// --- Pareto Frontier (ADR-011 / ADR-019) ---

const INITIAL_PARETO_FRONTIER: ParetoFrontierResult = {
  points: [
    { experimentId: '11111111-1111-1111-1111-111111111111', experimentName: 'homepage_recs_v2', objectiveX: 0.032, objectiveY: 0.85, objectiveXLabel: 'Engagement Lift', objectiveYLabel: 'Diversity Score', isPareto: true },
    { experimentId: '22222222-2222-2222-2222-222222222222', experimentName: 'search_ranking_v3', objectiveX: 0.022, objectiveY: 0.91, objectiveXLabel: 'Engagement Lift', objectiveYLabel: 'Diversity Score', isPareto: true },
    { experimentId: '33333333-3333-3333-3333-333333333333', experimentName: 'playback_buffer_opt', objectiveX: 0.015, objectiveY: 0.72, objectiveXLabel: 'Engagement Lift', objectiveYLabel: 'Diversity Score', isPareto: false },
    { experimentId: '44444444-4444-4444-4444-444444444444', experimentName: 'content_diversity_boost', objectiveX: 0.009, objectiveY: 0.95, objectiveXLabel: 'Engagement Lift', objectiveYLabel: 'Diversity Score', isPareto: true },
    { experimentId: '55555555-5555-5555-5555-555555555555', experimentName: 'retention_nudge', objectiveX: 0.028, objectiveY: 0.78, objectiveXLabel: 'Engagement Lift', objectiveYLabel: 'Diversity Score', isPareto: false },
  ],
  frontierIds: ['11111111-1111-1111-1111-111111111111', '22222222-2222-2222-2222-222222222222', '44444444-4444-4444-4444-444444444444'],
  computedAt: '2026-03-24T10:00:00Z',
};

export let SEED_PARETO_FRONTIER: ParetoFrontierResult = structuredClone(INITIAL_PARETO_FRONTIER);

// --- Meta-Experiment Results (ADR-013) ---

const META_EXP_ID = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';

const INITIAL_META_EXPERIMENT_RESULTS: MetaExperimentResult[] = [
  {
    experimentId: META_EXP_ID,
    variantResults: [
      {
        variantId: 'v-ctrl',
        variantName: 'control',
        banditType: 'THOMPSON_SAMPLING',
        bestArm: 'arm-a',
        bestArmRewardRate: 0.142,
        avgRewardRate: 0.118,
        explorationFraction: 0.15,
        ipwEffect: 0.0,
        ipwSe: 0.005,
        ipwCiLower: -0.0098,
        ipwCiUpper: 0.0098,
      },
      {
        variantId: 'v-treat',
        variantName: 'treatment',
        banditType: 'LINEAR_UCB',
        bestArm: 'arm-x',
        bestArmRewardRate: 0.168,
        avgRewardRate: 0.139,
        explorationFraction: 0.10,
        ipwEffect: 0.0210,
        ipwSe: 0.0072,
        ipwCiLower: 0.0069,
        ipwCiUpper: 0.0351,
      },
    ],
    overallWinner: 'treatment',
    cochranQPValue: 0.032,
    computedAt: '2026-03-24T12:00:00Z',
  },
];

export let SEED_META_EXPERIMENT_RESULTS: MetaExperimentResult[] = structuredClone(INITIAL_META_EXPERIMENT_RESULTS);

// --- Slate Heatmap (ADR-016 extension) ---

const SLATE_EXP_ID = 'cccccccc-cccc-cccc-cccc-cccccccccccc';

const INITIAL_SLATE_HEATMAP_RESULTS: SlateHeatmapResult[] = [
  {
    experimentId: SLATE_EXP_ID,
    items: ['item-a', 'item-b', 'item-c', 'item-d', 'item-e'],
    positions: [1, 2, 3, 4, 5],
    cells: [
      { itemId: 'item-a', position: 1, probability: 0.45 },
      { itemId: 'item-a', position: 2, probability: 0.25 },
      { itemId: 'item-a', position: 3, probability: 0.15 },
      { itemId: 'item-a', position: 4, probability: 0.10 },
      { itemId: 'item-a', position: 5, probability: 0.05 },
      { itemId: 'item-b', position: 1, probability: 0.20 },
      { itemId: 'item-b', position: 2, probability: 0.35 },
      { itemId: 'item-b', position: 3, probability: 0.22 },
      { itemId: 'item-b', position: 4, probability: 0.13 },
      { itemId: 'item-b', position: 5, probability: 0.10 },
      { itemId: 'item-c', position: 1, probability: 0.15 },
      { itemId: 'item-c', position: 2, probability: 0.18 },
      { itemId: 'item-c', position: 3, probability: 0.30 },
      { itemId: 'item-c', position: 4, probability: 0.22 },
      { itemId: 'item-c', position: 5, probability: 0.15 },
      { itemId: 'item-d', position: 1, probability: 0.12 },
      { itemId: 'item-d', position: 2, probability: 0.14 },
      { itemId: 'item-d', position: 3, probability: 0.20 },
      { itemId: 'item-d', position: 4, probability: 0.32 },
      { itemId: 'item-d', position: 5, probability: 0.22 },
      { itemId: 'item-e', position: 1, probability: 0.08 },
      { itemId: 'item-e', position: 2, probability: 0.08 },
      { itemId: 'item-e', position: 3, probability: 0.13 },
      { itemId: 'item-e', position: 4, probability: 0.23 },
      { itemId: 'item-e', position: 5, probability: 0.48 },
    ],
    computedAt: '2026-03-23T12:00:00Z',
  },
];

export let SEED_SLATE_HEATMAP_RESULTS: SlateHeatmapResult[] = structuredClone(INITIAL_SLATE_HEATMAP_RESULTS);

/** Reset seed data to initial state. Call in afterEach for test isolation. */
export function resetSeedData(): void {
  SEED_FLAGS = structuredClone(INITIAL_FLAGS);
  SEED_METRIC_DEFINITIONS = structuredClone(INITIAL_METRIC_DEFINITIONS);
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
  SEED_LAYERS = structuredClone(INITIAL_LAYERS);
  SEED_LAYER_ALLOCATIONS = structuredClone(INITIAL_LAYER_ALLOCATIONS);
  SEED_AUDIT_LOG = structuredClone(INITIAL_AUDIT_LOG);
  SEED_PROVIDER_HEALTH = structuredClone(INITIAL_PROVIDER_HEALTH);
  SEED_AVLM_RESULTS = structuredClone(INITIAL_AVLM_RESULTS);
  SEED_ADAPTIVE_N_RESULTS = structuredClone(INITIAL_ADAPTIVE_N_RESULTS);
  SEED_FEEDBACK_LOOP_RESULTS = structuredClone(INITIAL_FEEDBACK_LOOP_RESULTS);
  SEED_ONLINE_FDR_STATES = structuredClone(INITIAL_ONLINE_FDR_STATES);
  SEED_OPTIMAL_ALPHA = structuredClone(INITIAL_OPTIMAL_ALPHA);
  SEED_PORTFOLIO_ALLOCATION = structuredClone(INITIAL_PORTFOLIO_ALLOCATION);
  SEED_SLATE_OPE_RESULTS = structuredClone(INITIAL_SLATE_OPE_RESULTS);
  SEED_SWITCHBACK_RESULTS = structuredClone(INITIAL_SWITCHBACK_RESULTS);
  SEED_SYNTHETIC_CONTROL_RESULTS = structuredClone(INITIAL_SYNTHETIC_CONTROL_RESULTS);
  SEED_PORTFOLIO_METRICS = structuredClone(INITIAL_PORTFOLIO_METRICS);
  SEED_PARETO_FRONTIER = structuredClone(INITIAL_PARETO_FRONTIER);
  SEED_META_EXPERIMENT_RESULTS = structuredClone(INITIAL_META_EXPERIMENT_RESULTS);
  SEED_SLATE_HEATMAP_RESULTS = structuredClone(INITIAL_SLATE_HEATMAP_RESULTS);
}
