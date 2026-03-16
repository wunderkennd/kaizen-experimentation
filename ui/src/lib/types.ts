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
  interleavingConfig?: InterleavingConfig;
  sessionConfig?: SessionConfig;
  banditExperimentConfig?: BanditExperimentConfig;
  qoeConfig?: QoeConfig;
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
  interleavingConfig?: InterleavingConfig;
  sessionConfig?: SessionConfig;
  banditExperimentConfig?: BanditExperimentConfig;
  qoeConfig?: QoeConfig;
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

// --- GST Boundary Trajectory (M3.8) ---

export interface GstBoundaryPoint {
  look: number;
  informationFraction: number;
  boundaryZScore: number;
  observedZScore?: number;
}

export interface GstTrajectoryResult {
  experimentId: string;
  metricId: string;
  method: SequentialMethod;
  plannedLooks: number;
  overallAlpha: number;
  boundaryPoints: GstBoundaryPoint[];
  computedAt: string;
}

// --- Lorenz Curve (Interference M2.5 extension) ---

export interface LorenzCurvePoint {
  cumulativeContentFraction: number;
  cumulativeConsumptionFraction: number;
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
  sessionLevelResult?: SessionLevelResult;
  segmentResults?: SegmentResult[];
}

export interface SessionLevelResult {
  naiveSe: number;
  clusteredSe: number;
  designEffect: number;
  naivePValue: number;
  clusteredPValue: number;
}

export interface SrmResult {
  chiSquared: number;
  pValue: number;
  isMismatch: boolean;
  observedCounts: Record<string, number>;
  expectedCounts: Record<string, number>;
}

export interface SegmentResult {
  segment: LifecycleSegment;
  effect: number;
  ciLower: number;
  ciUpper: number;
  pValue: number;
  sampleSize: number;
}

export interface AnalysisResult {
  experimentId: string;
  metricResults: MetricResult[];
  srmResult: SrmResult;
  surrogateProjections?: SurrogateProjection[];
  cochranQPValue?: number;
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
  dailyEffects?: NoveltyDailyEffect[];
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
  treatmentLorenzCurve?: LorenzCurvePoint[];
  controlLorenzCurve?: LorenzCurvePoint[];
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
  /** Proto field — present in wire format, mapped to metricId/surrogateMetricId by adapter */
  modelId?: string;
  /** Proto field — present in wire format, not used directly by UI */
  variantId?: string;
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

// --- QoE Dashboard (M2.10) ---

export type QoeStatus = 'GOOD' | 'WARNING' | 'CRITICAL';

export interface QoeMetricSnapshot {
  metricId: string;
  label: string;
  controlValue: number;
  treatmentValue: number;
  unit: string;
  lowerIsBetter: boolean;
  warningThreshold: number;
  criticalThreshold: number;
  status: QoeStatus;
}

export interface QoeDashboardResult {
  experimentId: string;
  snapshots: QoeMetricSnapshot[];
  overallStatus: QoeStatus;
  computedAt: string;
}

// --- Novelty Daily Effects (M2.4 extension) ---

export interface NoveltyDailyEffect {
  day: number;
  observedEffect: number;
  fittedEffect: number;
}

// --- CATE / Lifecycle Segments (M4.1) ---

export type LifecycleSegment =
  | 'TRIAL'
  | 'NEW'
  | 'ESTABLISHED'
  | 'MATURE'
  | 'AT_RISK'
  | 'WINBACK';

export interface SubgroupEffect {
  segment: LifecycleSegment;
  effect: number;
  se: number;
  ciLower: number;
  ciUpper: number;
  pValueRaw: number;
  pValueAdjusted: number;
  isSignificant: boolean;
  nControl: number;
  nTreatment: number;
  controlMean: number;
  treatmentMean: number;
}

export interface HeterogeneityTest {
  qStatistic: number;
  df: number;
  pValue: number;
  iSquared: number;
  heterogeneityDetected: boolean;
}

export interface CateAnalysisResult {
  experimentId: string;
  metricId: string;
  globalAte: number;
  globalSe: number;
  globalCiLower: number;
  globalCiUpper: number;
  globalPValue: number;
  subgroupEffects: SubgroupEffect[];
  heterogeneity: HeterogeneityTest;
  nSubgroups: number;
  fdrThreshold: number;
  computedAt: string;
}
// --- Bandit Dashboard (M3.3) ---

// --- Layer Allocation (M6 bucket visualization) ---

export interface Layer {
  layerId: string;
  name: string;
  description: string;
  totalBuckets: number;
}

export interface LayerAllocation {
  allocationId: string;
  layerId: string;
  experimentId: string;
  startBucket: number;
  endBucket: number;
  activatedAt?: string;
  releasedAt?: string;
}

// --- Metric Definitions (M6 metric browser) ---

export type MetricType = 'MEAN' | 'PROPORTION' | 'RATIO' | 'COUNT' | 'PERCENTILE' | 'CUSTOM';

export interface MetricDefinition {
  metricId: string;
  name: string;
  description: string;
  type: MetricType;
  sourceEventType: string;
  numeratorEventType?: string;
  denominatorEventType?: string;
  percentile?: number;
  customSql?: string;
  lowerIsBetter: boolean;
  surrogateTargetMetricId?: string;
  isQoeMetric: boolean;
  cupedCovariateMetricId?: string;
  minimumDetectableEffect?: number;
}

export interface ListMetricDefinitionsResponse {
  metrics: MetricDefinition[];
  nextPageToken: string;
}

// --- Type-specific experiment config (wizard step 2) ---

export type InterleavingMethod = 'TEAM_DRAFT' | 'OPTIMIZED' | 'MULTILEAVE';
export type CreditAssignment = 'BINARY_WIN' | 'PROPORTIONAL' | 'WEIGHTED';

export interface InterleavingConfig {
  method: InterleavingMethod;
  algorithmIds: string[];
  creditAssignment: CreditAssignment;
  creditMetricEvent: string;
  maxListSize: number;
}

export interface SessionConfig {
  sessionIdAttribute: string;
  allowCrossSessionVariation: boolean;
  minSessionsPerUser: number;
}

export interface BanditExperimentConfig {
  algorithm: BanditAlgorithm;
  rewardMetricId: string;
  contextFeatureKeys: string[];
  minExplorationFraction: number;
  warmupObservations: number;
}

export interface QoeConfig {
  qoeMetrics: string[];
  deviceFilter: string;
}

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
