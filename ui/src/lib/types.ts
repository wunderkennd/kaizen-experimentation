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
  nextPageToken: string;
}

export interface CreateExperimentRequest {
  name: string;
  description: string;
  ownerEmail: string;
  type: ExperimentType;
  variants: Variant[];
  layerId: string;
  primaryMetricId: string;
  secondaryMetricIds: string[];
  guardrailConfigs: GuardrailConfig[];
  guardrailAction: GuardrailAction;
  sequentialTestConfig?: SequentialTestConfig;
  targetingRuleId?: string;
  isCumulativeHoldout: boolean;
}

export interface QueryLogEntry {
  experimentId: string;
  metricId: string;
  sqlText: string;
  rowCount: number;
  durationMs: number;
  computedAt?: string;
}

export interface SequentialResult {
  boundaryCrossed: boolean;
  alphaSpent: number;
  alphaRemaining: number;
  currentLook: number;
  adjustedPValue: number;
}

export interface MetricResult {
  metricId: string;
  variantId: string;
  controlMean: number;
  treatmentMean: number;
  absoluteEffect: number;
  relativeEffect: number;
  ciLower: number;
  ciUpper: number;
  pValue: number;
  isSignificant: boolean;
  cupedAdjustedEffect: number;
  cupedCiLower: number;
  cupedCiUpper: number;
  varianceReductionPct: number;
  sequentialResult?: SequentialResult;
}

export interface SrmResult {
  chiSquared: number;
  pValue: number;
  isMismatch: boolean;
  observedCounts: Record<string, number>;
  expectedCounts: Record<string, number>;
}

export interface AnalysisResult {
  experimentId: string;
  metricResults: MetricResult[];
  srmResult: SrmResult;
  surrogateProjections?: SurrogateProjection[];
  computedAt: string;
}

// --- Novelty Analysis (M2.4) ---

export interface NoveltyAnalysisResult {
  experimentId: string;
  metricId: string;
  noveltyDetected: boolean;
  rawTreatmentEffect: number;
  projectedSteadyStateEffect: number;
  noveltyAmplitude: number;
  decayConstantDays: number;
  isStabilized: boolean;
  daysUntilProjectedStability: number;
  computedAt: string;
}

// --- Interference Analysis (M2.5) ---

export interface TitleSpillover {
  contentId: string;
  treatmentWatchRate: number;
  controlWatchRate: number;
  pValue: number;
}

export interface InterferenceAnalysisResult {
  experimentId: string;
  interferenceDetected: boolean;
  jensenShannonDivergence: number;
  jaccardSimilarityTop100: number;
  treatmentGiniCoefficient: number;
  controlGiniCoefficient: number;
  treatmentCatalogCoverage: number;
  controlCatalogCoverage: number;
  spilloverTitles: TitleSpillover[];
  computedAt: string;
}

// --- Interleaving Analysis (M2.6) ---

export interface AlgorithmStrength {
  algorithmId: string;
  strength: number;
  ciLower: number;
  ciUpper: number;
}

export interface PositionAnalysis {
  position: number;
  algorithmEngagementRates: Record<string, number>;
}

export interface InterleavingAnalysisResult {
  experimentId: string;
  algorithmWinRates: Record<string, number>;
  signTestPValue: number;
  algorithmStrengths: AlgorithmStrength[];
  positionAnalyses: PositionAnalysis[];
  computedAt: string;
}

// --- Surrogate Projections (M2.10) ---

export interface SurrogateProjection {
  metricId: string;
  surrogateMetricId: string;
  projectedEffect: number;
  projectionCiLower: number;
  projectionCiUpper: number;
  calibrationRSquared: number;
}

// --- Cumulative Holdout (M2.10) ---

export interface HoldoutTimeSeriesPoint {
  date: string;
  cumulativeLift: number;
  ciLower: number;
  ciUpper: number;
  sampleSize: number;
}

export interface CumulativeHoldoutResult {
  experimentId: string;
  metricId: string;
  currentCumulativeLift: number;
  currentCiLower: number;
  currentCiUpper: number;
  isSignificant: boolean;
  timeSeries: HoldoutTimeSeriesPoint[];
  computedAt: string;
}

// --- Guardrail Breach History (M2.10) ---

export interface GuardrailBreachEvent {
  experimentId: string;
  metricId: string;
  variantId: string;
  currentValue: number;
  threshold: number;
  consecutiveBreachCount: number;
  action: 'ALERT' | 'AUTO_PAUSE';
  detectedAt: string;
}

export interface GuardrailStatusResult {
  experimentId: string;
  breaches: GuardrailBreachEvent[];
  isPaused: boolean;
}

// --- Bandit Dashboard (M3.3) ---

export type BanditAlgorithm = 'THOMPSON_SAMPLING' | 'LINEAR_UCB' | 'THOMPSON_LINEAR' | 'NEURAL_CONTEXTUAL';

export interface BanditArmStats {
  armId: string;
  name: string;
  selectionCount: number;
  rewardCount: number;
  rewardRate: number;
  assignmentProbability: number;
  alpha?: number;
  beta?: number;
  expectedReward?: number;
}

export interface RewardHistoryPoint {
  timestamp: string;
  armId: string;
  cumulativeReward: number;
  cumulativeSelections: number;
}

export interface BanditDashboardResult {
  experimentId: string;
  algorithm: BanditAlgorithm;
  totalRewardsProcessed: number;
  snapshotAt: string;
  arms: BanditArmStats[];
  isWarmup: boolean;
  warmupObservations: number;
  minExplorationFraction: number;
  rewardHistory: RewardHistoryPoint[];
}
