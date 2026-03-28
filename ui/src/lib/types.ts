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
  /** ADR-018: present when online FDR control is enabled for this experiment's program. */
  onlineFdrConfig?: OnlineFdrConfig;
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

// --- IPW-adjusted results (M4a bandit experiments) ---

export interface IpwResult {
  effect: number;
  se: number;
  ciLower: number;
  ciUpper: number;
  pValue: number;
  isSignificant: boolean;
  nClipped: number;
  effectiveSampleSize: number;
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
  ipwResult?: IpwResult;
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
  /** ADR-018: present when e-value framework is enabled for this experiment. */
  eValueResult?: EValueResult;
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

// --- Feature Flags (M7) ---

export type FlagType = 'BOOLEAN' | 'STRING' | 'NUMERIC' | 'JSON';

export interface FlagVariant {
  variantId: string;
  value: string;
  trafficFraction: number;
}

export interface Flag {
  flagId: string;
  name: string;
  description: string;
  type: FlagType;
  defaultValue: string;
  enabled: boolean;
  rolloutPercentage: number;
  variants: FlagVariant[];
  targetingRuleId?: string;
}

export interface ListFlagsResponse {
  flags: Flag[];
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

export type RewardCompositionMethod =
  | 'WEIGHTED_SCALARIZATION'
  | 'EPSILON_CONSTRAINT'
  | 'TCHEBYCHEFF';

export interface RewardObjective {
  metricId: string;
  weight: number;
  /** Floor constraint for EPSILON_CONSTRAINT (minimum normalized value). */
  floor: number;
  isPrimary: boolean;
}

export interface BanditArmConstraint {
  armId: string;
  minFraction: number;
  maxFraction: number;
}

export interface BanditGlobalConstraint {
  label: string;
  /** Coefficient for each arm keyed by armId. */
  coefficients: Record<string, number>;
  /** Right-hand side: Σ(coeff_i × p_i) <= rhs. */
  rhs: number;
}

export interface BanditExperimentConfig {
  algorithm: BanditAlgorithm;
  rewardMetricId: string;
  contextFeatureKeys: string[];
  minExplorationFraction: number;
  warmupObservations: number;
  /** Multi-objective reward objectives (ADR-011). When non-empty, rewardMetricId is ignored. */
  rewardObjectives?: RewardObjective[];
  compositionMethod?: RewardCompositionMethod;
  /** Per-arm traffic bounds for LP post-processing layer (ADR-012). */
  armConstraints?: BanditArmConstraint[];
  /** General linear constraints across arms (ADR-012). */
  globalConstraints?: BanditGlobalConstraint[];
}

export interface QoeConfig {
  qoeMetrics: string[];
  deviceFilter: string;
}

// --- Audit Log (M6 audit viewer) ---

export type AuditAction =
  | 'CREATED'
  | 'UPDATED'
  | 'STARTED'
  | 'PAUSED'
  | 'RESUMED'
  | 'CONCLUDED'
  | 'ARCHIVED'
  | 'GUARDRAIL_BREACH'
  | 'CONFIG_CHANGED';

export interface AuditLogEntry {
  entryId: string;
  experimentId: string;
  experimentName: string;
  action: AuditAction;
  actorEmail: string;
  timestamp: string;
  details: string;
  previousValue?: string;
  newValue?: string;
}

export interface ListAuditLogResponse {
  entries: AuditLogEntry[];
  nextPageToken: string;
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

/** Per-arm per-objective weighted contribution for stacked bar chart (ADR-011). */
export interface ArmObjectiveBreakdown {
  armId: string;
  armName: string;
  /** metricId → weighted normalized contribution (weight × normalized_reward). */
  objectiveContributions: Record<string, number>;
  composedReward: number;
}

/** LP constraint current status row for ConstraintStatusTable (ADR-012). */
export interface ConstraintStatus {
  label: string;
  currentValue: number;
  limit: number;
  isSatisfied: boolean;
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
  /** Multi-objective reward breakdown per arm (ADR-011). Present when reward_objectives non-empty. */
  objectiveBreakdowns?: ArmObjectiveBreakdown[];
  /** LP constraint current statuses (ADR-012). Present when global_constraints non-empty. */
  constraintStatuses?: ConstraintStatus[];
}

// --- AVLM Confidence Sequence (ADR-015) ---

export interface AvlmBoundaryPoint {
  look: number;
  informationFraction: number; // 0–1
  upperBound: number;          // upper confidence sequence boundary
  lowerBound: number;          // lower confidence sequence boundary
  estimate: number;            // CUPED-adjusted point estimate
  estimateRaw: number;         // unadjusted point estimate
}

export interface AvlmResult {
  experimentId: string;
  metricId: string;
  boundaryPoints: AvlmBoundaryPoint[];
  varianceReductionPct: number;
  isConclusive: boolean;
  conclusiveLook?: number;
  finalEstimate: number;
  finalCiLower: number;
  finalCiUpper: number;
  computedAt: string;
}

// --- Adaptive Sample Size (ADR-020) ---

export type AdaptiveNZone = 'FAVORABLE' | 'PROMISING' | 'FUTILE' | 'INCONCLUSIVE';

export interface AdaptiveNTimelinePoint {
  date: string;
  estimatedN: number;
}

export interface AdaptiveNResult {
  experimentId: string;
  zone: AdaptiveNZone;
  currentN: number;
  plannedN: number;
  recommendedN?: number;
  conditionalPower: number;
  projectedConclusionDate?: string;
  extensionDays?: number;
  timelineProjection: AdaptiveNTimelinePoint[];
  computedAt: string;
}

// --- E-Value Framework (ADR-018) ---

export interface EValueResult {
  /** The e-value. > 1 is evidence against null; >= 1/alpha rejects at level alpha. */
  eValue: number;
  /** Natural log of e-value (numerical stability for products). */
  logEValue: number;
  /** Implied significance level: 1/eValue. Comparable to a p-value, different semantics. */
  impliedLevel: number;
  /** Whether the null is rejected: eValue >= 1/alpha. */
  reject: boolean;
  /** Significance level used (e.g., 0.05). */
  alpha: number;
}

export type OnlineFdrStrategy = 'E_LOND' | 'E_BH';

/** Configuration stored on the experiment when online FDR control is enabled. */
export interface OnlineFdrConfig {
  targetAlpha: number;
  initialWealth: number;
  strategy: OnlineFdrStrategy;
}

/** Current state of the platform-level OnlineFdrController for this experiment's program. */
export interface OnlineFdrState {
  experimentId: string;
  alphaWealth: number;
  initialWealth: number;
  numTested: number;
  numRejected: number;
  currentFdr: number;
  computedAt: string;
}

// --- Feedback Loop Analysis ---

export interface RetrainingEvent {
  eventId: string;
  retrainedAt: string;
  triggerReason: string;
  modelVersion: string;
}

export interface FeedbackLoopPrePost {
  date: string;
  preEffect: number;
  postEffect: number;
}

export interface ContaminationPoint {
  date: string;
  contaminationFraction: number;
}

export type MitigationSeverity = 'HIGH' | 'MEDIUM' | 'LOW';

export interface MitigationRecommendation {
  recommendationId: string;
  severity: MitigationSeverity;
  title: string;
  description: string;
  action: string;
}

export interface FeedbackLoopResult {
  experimentId: string;
  retrainingEvents: RetrainingEvent[];
  prePostComparison: FeedbackLoopPrePost[];
  contaminationTimeline: ContaminationPoint[];
  rawEstimate: number;
  biasCorrectedEstimate: number;
  biasCorrectedCiLower: number;
  biasCorrectedCiUpper: number;
  contaminationFraction: number;
  recommendations: MitigationRecommendation[];
  computedAt: string;
}

// --- Portfolio Optimization (ADR-019) ---

export interface PortfolioExperiment {
  experimentId: string;
  name: string;
  effectSize: number;
  variance: number;
  allocatedTrafficPct: number;
  priorityScore: number;
  userSegments: string[];
}

export interface PortfolioAllocationResult {
  experiments: PortfolioExperiment[];
  totalAllocatedPct: number;
  computedAt: string;
}

// --- Provider Health (ADR-014) ---

export interface ProviderHealthPoint {
  date: string;
  catalogCoverage: number;
  providerGini: number;
  longTailImpressionShare: number;
}

export interface ProviderHealthSeries {
  providerId: string;
  providerName: string;
  experimentId: string;
  experimentName: string;
  points: ProviderHealthPoint[];
}

export interface ProviderInfo {
  providerId: string;
  providerName: string;
}

export interface ProviderHealthResult {
  series: ProviderHealthSeries[];
  providers: ProviderInfo[];
  computedAt: string;
}
