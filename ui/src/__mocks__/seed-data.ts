import type { Experiment, QueryLogEntry } from '@/lib/types';

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

/** Mutable copy of seed data — MSW handlers mutate this in-place. */
export let SEED_EXPERIMENTS: Experiment[] = structuredClone(INITIAL_EXPERIMENTS);
export let SEED_QUERY_LOG: Record<string, QueryLogEntry[]> = structuredClone(INITIAL_QUERY_LOG);

/** Reset seed data to initial state. Call in afterEach for test isolation. */
export function resetSeedData(): void {
  SEED_EXPERIMENTS = structuredClone(INITIAL_EXPERIMENTS);
  SEED_QUERY_LOG = structuredClone(INITIAL_QUERY_LOG);
}
