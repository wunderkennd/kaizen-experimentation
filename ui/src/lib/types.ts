/** Local TS types mirroring proto/experimentation/common/v1/experiment.proto.
 *  camelCase field names match ConnectRPC's default JSON serialization.
 *  When gen/ts/ is generated, re-export from there instead. */

export type ExperimentState =
  | 'DRAFT'
  | 'STARTING'
  | 'RUNNING'
  | 'CONCLUDING'
  | 'CONCLUDED'
  | 'ARCHIVED';

export type ExperimentType =
  | 'AB'
  | 'MULTIVARIATE'
  | 'INTERLEAVING'
  | 'SESSION_LEVEL'
  | 'PLAYBACK_QOE'
  | 'MAB'
  | 'CONTEXTUAL_BANDIT'
  | 'CUMULATIVE_HOLDOUT';

export type GuardrailAction = 'AUTO_PAUSE' | 'ALERT_ONLY';

export type SequentialMethod = 'MSPRT' | 'GST_OBF' | 'GST_POCOCK';

export interface Variant {
  variantId: string;
  name: string;
  trafficFraction: number;
  isControl: boolean;
  payloadJson: string;
}

export interface GuardrailConfig {
  metricId: string;
  threshold: number;
  consecutiveBreachesRequired: number;
}

export interface SequentialTestConfig {
  method: SequentialMethod;
  plannedLooks: number;
  overallAlpha: number;
}

export interface Experiment {
  experimentId: string;
  name: string;
  description: string;
  ownerEmail: string;
  type: ExperimentType;
  state: ExperimentState;
  variants: Variant[];
  layerId: string;
  hashSalt: string;
  primaryMetricId: string;
  secondaryMetricIds: string[];
  guardrailConfigs: GuardrailConfig[];
  guardrailAction: GuardrailAction;
  sequentialTestConfig?: SequentialTestConfig;
  targetingRuleId?: string;
  surrogateModelId?: string;
  isCumulativeHoldout: boolean;
  createdAt: string;
  startedAt?: string;
  concludedAt?: string;
}

export interface ListExperimentsResponse {
  experiments: Experiment[];
  totalCount: number;
}
