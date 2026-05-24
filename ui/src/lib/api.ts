import type {
  AnalysisResult, CreateExperimentRequest, Experiment, ListExperimentsResponse,
  QueryLogEntry, NoveltyAnalysisResult, InterferenceAnalysisResult, InterleavingAnalysisResult,
  BanditDashboardResult, CumulativeHoldoutResult, GuardrailStatusResult, QoeDashboardResult,
  GstTrajectoryResult, CateAnalysisResult, Layer, LayerAllocation,
  SurrogateProjection, SrmResult, MetricResult, SegmentResult, IpwResult, EquivalenceResult,
  MetricDefinition, ListMetricDefinitionsResponse,
  MetricTypeConfig, FilteredMeanConfig, CompositeConfig, CompositeOperand, WindowedCountConfig,
  Flag, FlagType, ListFlagsResponse,
  InterleavingConfig, SessionConfig, BanditExperimentConfig, QoeConfig,
  AuditLogEntry, AuditAction, ListAuditLogResponse,
  ProviderHealthResult,
  AvlmResult, AdaptiveNResult, FeedbackLoopResult,
  OnlineFdrState, OptimalAlphaRecommendation,
  PortfolioAllocationResult, PortfolioMetricsResult,
  ParetoFrontierResult,
  MetaExperimentResult,
  SlateAssignmentResponse, SlateOpeResult, SlateHeatmapResult,
  SwitchbackResult, SyntheticControlResult,
} from './types';
import type {
  ExperimentState, ExperimentType, MetricType, LifecycleSegment,
  MetricStakeholder, MetricAggregationLevel,
} from './types';
import { CompositeOperator } from './types';

// In the browser, default to relative proxy paths (Next.js rewrites handle CORS).
// In tests, vitest.config.ts sets NEXT_PUBLIC_*_URL to absolute URLs so MSW can intercept.
// In production, set NEXT_PUBLIC_*_URL env vars to point to your backends directly.
const MGMT_URL = process.env.NEXT_PUBLIC_MANAGEMENT_URL || '/api/rpc/management';
const MGMT_SVC = 'experimentation.management.v1.ExperimentManagementService';

const METRICS_URL = process.env.NEXT_PUBLIC_METRICS_URL || '/api/rpc/metrics';
const METRICS_SVC = 'experimentation.metrics.v1.MetricComputationService';

const ANALYSIS_URL = process.env.NEXT_PUBLIC_ANALYSIS_URL || '/api/rpc/analysis';
const ANALYSIS_SVC = 'experimentation.analysis.v1.AnalysisService';

const BANDIT_URL = process.env.NEXT_PUBLIC_BANDIT_URL || '/api/rpc/bandit';
const BANDIT_SVC = 'experimentation.bandit.v1.BanditPolicyService';

const FLAGS_URL = process.env.NEXT_PUBLIC_FLAGS_URL || '/api/rpc/flags';
const FLAGS_SVC = 'experimentation.flags.v1.FeatureFlagService';

const ASSIGNMENT_URL = process.env.NEXT_PUBLIC_ASSIGNMENT_URL || '/api/rpc/assignment';
const ASSIGNMENT_SVC = 'experimentation.assignment.v1.AssignmentService';

// --- Auth header injection ---
let _authEmail = '';
let _authRole = '';

/** Set auth credentials injected into all RPC calls. Called by AuthProvider. */
export function setApiAuth(email: string, role: string): void {
  _authEmail = email;
  _authRole = role;
}

// --- In-memory request cache with TTL ---
interface CacheEntry<T> { data: T; expiresAt: number; }
const _cache = new Map<string, CacheEntry<unknown>>();
const DEFAULT_TTL_MS = 30_000;

function getCacheKey(baseUrl: string, service: string, method: string, request: unknown): string {
  return `${baseUrl}/${service}/${method}:${JSON.stringify(request)}`;
}

/** Clear the in-memory RPC cache. Call in test teardown. */
export function clearApiCache(): void {
  _cache.clear();
}

/** Default timeout for RPC calls in milliseconds. */
export const API_TIMEOUT_MS = 10_000;

/** Base delay for retry backoff in milliseconds. */
const RETRY_BASE_DELAY_MS = 500;

export class RpcError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = 'RpcError';
    this.status = status;
  }
}

/** Check if an error represents a 403 Permission Denied response. */
export function isPermissionDenied(error: unknown): boolean {
  return error instanceof RpcError && error.status === 403;
}

/** Parse ConnectRPC error response for a human-readable message. */
async function parseRpcError(res: Response, method: string): Promise<string> {
  try {
    const body = await res.json();
    if (body.message) return body.message;
    if (body.error) return body.error;
  } catch {
    // body wasn't JSON
  }
  return `RPC ${method} failed: ${res.status}`;
}

interface CallRpcOptions {
  skipCache?: boolean;
  clearCacheOnSuccess?: boolean;
  timeoutMs?: number;
  retries?: number;
}

/** Returns true for errors caused by network-level failures (no connection, timeout). */
export function isNetworkError(err: unknown): boolean {
  if (err instanceof DOMException && err.name === 'AbortError') return true;
  if (err instanceof TypeError) return true; // fetch throws TypeError on network failure
  return false;
}

async function callRpc<Req, Res>(
  baseUrl: string,
  service: string,
  method: string,
  request: Req,
  options: CallRpcOptions = {},
): Promise<Res> {
  const cacheKey = getCacheKey(baseUrl, service, method, request);

  // Check cache for read-only calls
  if (!options.skipCache) {
    const cached = _cache.get(cacheKey);
    if (cached && cached.expiresAt > Date.now()) {
      return cached.data as Res;
    }
  }

  const timeoutMs = options.timeoutMs ?? API_TIMEOUT_MS;
  // Default: retry once for read-like calls (no clearCacheOnSuccess), zero for mutations
  const maxRetries = options.retries ?? (options.clearCacheOnSuccess ? 0 : 1);

  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (_authEmail) headers['X-User-Email'] = _authEmail;
  if (_authRole) headers['X-User-Role'] = _authRole;

  let lastError: unknown;
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    // Backoff before retry (not on first attempt)
    if (attempt > 0) {
      await new Promise((r) => setTimeout(r, RETRY_BASE_DELAY_MS * attempt));
    }

    // Use Promise.race for timeout instead of AbortController signal
    // to avoid jsdom/Node AbortSignal incompatibility in tests.
    let timer: ReturnType<typeof setTimeout>;
    const timeoutPromise = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject(new DOMException('Request timed out', 'AbortError')),
        timeoutMs,
      );
    });

    try {
      const res = await Promise.race([
        fetch(`${baseUrl}/${service}/${method}`, {
          method: 'POST',
          headers,
          body: JSON.stringify(request),
        }),
        timeoutPromise,
      ]);
      clearTimeout(timer!);

      if (!res.ok) {
        // Never retry HTTP errors — only network-level failures
        throw new RpcError(await parseRpcError(res, method), res.status);
      }
      const data: Res = await res.json();

      // Cache read-only responses; clear cache on mutating calls
      if (options.clearCacheOnSuccess) {
        _cache.clear();
      } else if (!options.skipCache) {
        _cache.set(cacheKey, { data, expiresAt: Date.now() + DEFAULT_TTL_MS });
      }

      return data;
    } catch (err) {
      clearTimeout(timer!);
      // Never retry RpcError (server-decided 4xx/5xx)
      if (err instanceof RpcError) throw err;
      lastError = err;
      // Only retry on network-level failures
      if (!isNetworkError(err) || attempt === maxRetries) throw err;
    }
  }

  // Should never reach here, but satisfy TypeScript
  throw lastError;
}

/**
 * Like callRpc but accepts an external AbortSignal (for cancel-in-flight).
 *
 * The caller (linter) manages both the debounce timer and the AbortController;
 * we just honour the signal on the fetch call. Note: this bypasses the
 * Promise.race timeout pattern used by callRpc because the linter already
 * sets a manual timeout on the same controller.
 */
async function callRpcWithSignal<Req, Res>(
  baseUrl: string,
  service: string,
  method: string,
  request: Req,
  options: Omit<CallRpcOptions, 'timeoutMs'>,
  signal: AbortSignal,
): Promise<Res> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (_authEmail) headers['X-User-Email'] = _authEmail;
  if (_authRole) headers['X-User-Role'] = _authRole;

  const res = await fetch(`${baseUrl}/${service}/${method}`, {
    method: 'POST',
    headers,
    body: JSON.stringify(request),
    signal,
  });

  if (!res.ok) {
    throw new RpcError(await parseRpcError(res, method), res.status);
  }

  if (options.clearCacheOnSuccess) {
    _cache.clear();
  }

  return res.json() as Promise<Res>;
}

/** Strip proto enum prefix if present. e.g. "EXPERIMENT_STATE_DRAFT" → "DRAFT" */
function stripEnumPrefix(value: string, prefix: string): string {
  return value.startsWith(prefix) ? value.slice(prefix.length) : value;
}

/** Convert proto JSON experiment to local Experiment type. */
function adaptExperiment(proto: Record<string, unknown>): Experiment {
  const state = stripEnumPrefix(
    (proto.state as string) || 'DRAFT',
    'EXPERIMENT_STATE_',
  ) as ExperimentState;

  const type = stripEnumPrefix(
    (proto.type as string) || 'AB',
    'EXPERIMENT_TYPE_',
  ) as ExperimentType;

  return {
    experimentId: proto.experimentId as string,
    name: proto.name as string,
    description: (proto.description as string) || '',
    ownerEmail: (proto.ownerEmail as string) || '',
    type,
    state,
    variants: (proto.variants as Experiment['variants']) || [],
    layerId: (proto.layerId as string) || '',
    hashSalt: (proto.hashSalt as string) || '',
    primaryMetricId: (proto.primaryMetricId as string) || '',
    secondaryMetricIds: (proto.secondaryMetricIds as string[]) || [],
    guardrailConfigs: (proto.guardrailConfigs as Experiment['guardrailConfigs']) || [],
    guardrailAction: stripEnumPrefix(
      (proto.guardrailAction as string) || 'AUTO_PAUSE',
      'GUARDRAIL_ACTION_',
    ) as Experiment['guardrailAction'],
    sequentialTestConfig: proto.sequentialTestConfig as Experiment['sequentialTestConfig'],
    targetingRuleId: proto.targetingRuleId as string | undefined,
    surrogateModelId: proto.surrogateModelId as string | undefined,
    isCumulativeHoldout: (proto.isCumulativeHoldout as boolean) || false,
    interleavingConfig: proto.interleavingConfig as InterleavingConfig | undefined,
    sessionConfig: proto.sessionConfig as SessionConfig | undefined,
    banditExperimentConfig: proto.banditExperimentConfig as BanditExperimentConfig | undefined,
    qoeConfig: proto.qoeConfig as QoeConfig | undefined,
    onlineFdrConfig: proto.onlineFdrConfig as Experiment['onlineFdrConfig'],
    createdAt: (proto.createdAt as string) || '',
    startedAt: proto.startedAt as string | undefined,
    concludedAt: proto.concludedAt as string | undefined,
  };
}

export interface ListExperimentsFilters {
  stateFilter?: ExperimentState;
  typeFilter?: ExperimentType;
  ownerEmailFilter?: string;
  pageSize?: number;
  pageToken?: string;
}

export async function listExperiments(filters?: ListExperimentsFilters): Promise<ListExperimentsResponse> {
  const request: Record<string, unknown> = {};
  if (filters?.stateFilter) {
    request.stateFilter = `EXPERIMENT_STATE_${filters.stateFilter}`;
  }
  if (filters?.typeFilter) {
    request.typeFilter = `EXPERIMENT_TYPE_${filters.typeFilter}`;
  }
  if (filters?.ownerEmailFilter) {
    request.ownerEmailFilter = filters.ownerEmailFilter;
  }
  if (filters?.pageSize) {
    request.pageSize = filters.pageSize;
  }
  if (filters?.pageToken) {
    request.pageToken = filters.pageToken;
  }

  const raw = await callRpc<Record<string, unknown>, { experiments?: Record<string, unknown>[]; nextPageToken?: string }>(
    MGMT_URL, MGMT_SVC, 'ListExperiments', request,
  );
  return {
    experiments: (raw.experiments || []).map(adaptExperiment),
    nextPageToken: raw.nextPageToken || '',
  };
}

export async function getExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'GetExperiment', { experimentId: id },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function updateExperiment(experiment: Experiment): Promise<Experiment> {
  const raw = await callRpc<{ experiment: Experiment }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'UpdateExperiment', { experiment },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function startExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'StartExperiment', { experimentId: id },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function concludeExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'ConcludeExperiment', { experimentId: id },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function archiveExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'ArchiveExperiment', { experimentId: id },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function pauseExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'PauseExperiment', { experimentId: id },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function resumeExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'ResumeExperiment', { experimentId: id },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function getQueryLog(experimentId: string, metricId?: string): Promise<QueryLogEntry[]> {
  const raw = await callRpc<{ experimentId: string; metricId?: string }, { entries?: QueryLogEntry[] }>(
    METRICS_URL, METRICS_SVC, 'GetQueryLog', { experimentId, ...(metricId ? { metricId } : {}) },
  );
  return raw.entries || [];
}

export async function exportNotebook(experimentId: string): Promise<{ content: string; filename: string }> {
  const raw = await callRpc<{ experimentId: string }, { content: string; filename: string }>(
    METRICS_URL, METRICS_SVC, 'ExportNotebook', { experimentId },
    { skipCache: true },
  );
  return raw;
}

export async function createExperiment(request: CreateExperimentRequest): Promise<Experiment> {
  const raw = await callRpc<CreateExperimentRequest, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'CreateExperiment', request,
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

/** Coerce proto3 int64 string values to numbers in a Record<string, string|number>. */
function coerceInt64Map(map: Record<string, string | number> | undefined): Record<string, number> {
  if (!map) return {};
  const result: Record<string, number> = {};
  for (const [k, v] of Object.entries(map)) {
    result[k] = typeof v === 'string' ? Number(v) : v;
  }
  return result;
}

/** Adapt proto SurrogateProjection to UI type.
 *  Proto has experimentId/variantId/modelId; UI needs metricId/surrogateMetricId.
 *  Uses modelId as metricId fallback when metricId is absent. */
function adaptSurrogateProjection(proto: Record<string, unknown>): SurrogateProjection {
  return {
    metricId: (proto.metricId as string) || (proto.modelId as string) || '',
    surrogateMetricId: (proto.surrogateMetricId as string) || (proto.variantId as string) || '',
    projectedEffect: (proto.projectedEffect as number) || 0,
    projectionCiLower: (proto.projectionCiLower as number) || 0,
    projectionCiUpper: (proto.projectionCiUpper as number) || 0,
    calibrationRSquared: (proto.calibrationRSquared as number) || 0,
    modelId: proto.modelId as string | undefined,
    variantId: proto.variantId as string | undefined,
  };
}

/** Adapt proto SegmentResult — coerce int64 sampleSize, strip enum prefix. */
function adaptSegmentResult(proto: Record<string, unknown>): SegmentResult {
  return {
    segment: stripEnumPrefix((proto.segment as string) || '', 'LIFECYCLE_SEGMENT_') as LifecycleSegment,
    effect: (proto.effect as number) || 0,
    ciLower: (proto.ciLower as number) || 0,
    ciUpper: (proto.ciUpper as number) || 0,
    pValue: (proto.pValue as number) || 0,
    sampleSize: typeof proto.sampleSize === 'string' ? Number(proto.sampleSize) : (proto.sampleSize as number) || 0,
  };
}

/** Adapt proto SrmResult — coerce int64 map values to numbers. */
function adaptSrmResult(proto: Record<string, unknown>): SrmResult {
  return {
    chiSquared: (proto.chiSquared as number) || 0,
    pValue: (proto.pValue as number) || 0,
    isMismatch: (proto.isMismatch as boolean) || false,
    observedCounts: coerceInt64Map(proto.observedCounts as Record<string, string | number> | undefined),
    expectedCounts: coerceInt64Map(proto.expectedCounts as Record<string, string | number> | undefined),
  };
}

/** Adapt proto IpwResult — default proto3 zero-omitted fields to 0/false. */
function adaptIpwResult(proto: Record<string, unknown>): IpwResult {
  return {
    effect: (proto.effect as number) || 0,
    se: (proto.se as number) || 0,
    ciLower: (proto.ciLower as number) || 0,
    ciUpper: (proto.ciUpper as number) || 0,
    pValue: (proto.pValue as number) || 0,
    isSignificant: (proto.isSignificant as boolean) || false,
    nClipped: (proto.nClipped as number) || 0,
    effectiveSampleSize: (proto.effectiveSampleSize as number) || 0,
  };
}

/** ADR-027: Adapt proto EquivalenceResult — coerce numeric defaults. */
function adaptEquivalenceResult(proto: Record<string, unknown>): EquivalenceResult {
  return {
    pointEstimate: (proto.pointEstimate as number) || 0,
    stdError: (proto.stdError as number) || 0,
    df: (proto.df as number) || 0,
    pLower: (proto.pLower as number) || 0,
    pUpper: (proto.pUpper as number) || 0,
    pTost: (proto.pTost as number) || 0,
    ciLower: (proto.ciLower as number) || 0,
    ciUpper: (proto.ciUpper as number) || 0,
    equivalent: (proto.equivalent as boolean) || false,
    delta: (proto.delta as number) || 0,
    controlMean: (proto.controlMean as number) || 0,
    treatmentMean: (proto.treatmentMean as number) || 0,
    achievedPower: proto.achievedPower as number | undefined,
  };
}

/** Adapt proto MetricResult — coerce segmentResults int64 fields + IPW + equivalence. */
function adaptMetricResult(proto: Record<string, unknown>): MetricResult {
  const raw = proto as unknown as MetricResult & {
    segmentResults?: Record<string, unknown>[];
    ipwResult?: Record<string, unknown>;
    equivalenceResult?: Record<string, unknown>;
  };
  return {
    ...raw,
    segmentResults: raw.segmentResults?.map(adaptSegmentResult),
    ipwResult: raw.ipwResult ? adaptIpwResult(raw.ipwResult) : undefined,
    equivalenceResult: raw.equivalenceResult ? adaptEquivalenceResult(raw.equivalenceResult) : undefined,
  };
}

/** Adapt raw proto AnalysisResult to UI AnalysisResult type. */
function adaptAnalysisResult(raw: Record<string, unknown>): AnalysisResult {
  return {
    experimentId: (raw.experimentId as string) || '',
    metricResults: ((raw.metricResults as Record<string, unknown>[]) || []).map(adaptMetricResult),
    srmResult: adaptSrmResult((raw.srmResult as Record<string, unknown>) || {}),
    surrogateProjections: (raw.surrogateProjections as Record<string, unknown>[])?.map(adaptSurrogateProjection),
    cochranQPValue: raw.cochranQPValue as number | undefined,
    eValueResult: raw.eValueResult as AnalysisResult['eValueResult'],
    computedAt: (raw.computedAt as string) || '',
  };
}

export async function getAnalysisResult(experimentId: string): Promise<AnalysisResult> {
  const raw = await callRpc<{ experimentId: string }, Record<string, unknown>>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetAnalysisResult', { experimentId },
  );
  return adaptAnalysisResult(raw);
}

export async function getNoveltyAnalysis(experimentId: string): Promise<NoveltyAnalysisResult> {
  return callRpc<{ experimentId: string }, NoveltyAnalysisResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetNoveltyAnalysis', { experimentId },
  );
}

export async function getInterferenceAnalysis(experimentId: string): Promise<InterferenceAnalysisResult> {
  return callRpc<{ experimentId: string }, InterferenceAnalysisResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetInterferenceAnalysis', { experimentId },
  );
}

export async function getInterleavingAnalysis(experimentId: string): Promise<InterleavingAnalysisResult> {
  return callRpc<{ experimentId: string }, InterleavingAnalysisResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetInterleavingAnalysis', { experimentId },
  );
}

export async function getBanditDashboard(experimentId: string): Promise<BanditDashboardResult> {
  return callRpc<{ experimentId: string }, BanditDashboardResult>(
    BANDIT_URL, BANDIT_SVC, 'GetBanditDashboard', { experimentId },
  );
}

export async function getCumulativeHoldoutResult(experimentId: string): Promise<CumulativeHoldoutResult> {
  return callRpc<{ experimentId: string }, CumulativeHoldoutResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetCumulativeHoldoutResult', { experimentId },
  );
}

export async function getGstTrajectory(experimentId: string, metricId: string): Promise<GstTrajectoryResult> {
  const raw = await callRpc<{ experimentId: string; metricId: string }, GstTrajectoryResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetGstTrajectory', { experimentId, metricId },
  );
  return {
    ...raw,
    method: stripEnumPrefix(raw.method, 'SEQUENTIAL_METHOD_') as GstTrajectoryResult['method'],
  };
}

export async function getQoeDashboard(experimentId: string): Promise<QoeDashboardResult> {
  return callRpc<{ experimentId: string }, QoeDashboardResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetQoeDashboard', { experimentId },
  );
}

export async function getGuardrailStatus(experimentId: string): Promise<GuardrailStatusResult> {
  return callRpc<{ experimentId: string }, GuardrailStatusResult>(
    MGMT_URL, MGMT_SVC, 'GetGuardrailStatus', { experimentId },
  );
}

export async function getCateAnalysis(experimentId: string): Promise<CateAnalysisResult> {
  const raw = await callRpc<{ experimentId: string }, CateAnalysisResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetCateAnalysis', { experimentId },
  );
  return {
    ...raw,
    subgroupEffects: (raw.subgroupEffects || []).map((sg) => ({
      ...sg,
      segment: stripEnumPrefix(sg.segment, 'LIFECYCLE_SEGMENT_') as CateAnalysisResult['subgroupEffects'][number]['segment'],
    })),
  };
}

export async function getLayer(layerId: string): Promise<Layer> {
  return callRpc<{ layerId: string }, Layer>(
    MGMT_URL, MGMT_SVC, 'GetLayer', { layerId },
  );
}

export async function getLayerAllocations(
  layerId: string,
  includeReleased = false,
): Promise<LayerAllocation[]> {
  const raw = await callRpc<
    { layerId: string; includeReleased: boolean },
    { allocations?: LayerAllocation[] }
  >(MGMT_URL, MGMT_SVC, 'GetLayerAllocations', { layerId, includeReleased });
  return raw.allocations || [];
}

// ---- MetricDefinition wire-format helpers ----
//
// `adaptMetricDefinition` (wire → local) and `marshalMetricDefinition`
// (local → wire) are inverses. ADR-026 Phase 1 (#434) introduces the
// `type_config` proto oneof; the proto JSON encoding nests the populated
// case under its camelCase field name (`filteredMean` | `composite` |
// `windowedCount`) and leaves the others absent.
//
// Encoding choices on the local→wire path:
//   * `MetricType`: stays a string ('MEAN', 'FILTERED_MEAN', ...) — the M5
//     Rust handler accepts proto JSON enum string form, prefixed with
//     `METRIC_TYPE_`. Matches `typeFilter` in `listMetricDefinitions`.
//   * `CompositeOperator`: emitted as an integer (proto enum ordinal). The
//     Rust validator does `CompositeOperator::try_from(cfg.operator)`; the
//     int form avoids the BUG-0003 silent string-default trap from #552.
//   * Empty strings / 0.0 numerics in `CompositeOperand.weight`, etc., are
//     left in place (proto3 scalar defaults are the wire representation).

/** Decode the proto JSON `type_config` oneof into the local discriminated union. */
function adaptMetricTypeConfig(
  proto: Record<string, unknown>,
): MetricTypeConfig | undefined {
  const fm = proto.filteredMean as Record<string, unknown> | undefined;
  if (fm) {
    return {
      case: 'filteredMean',
      value: {
        filterSql: (fm.filterSql as string) || '',
        valueColumn: (fm.valueColumn as string) || '',
      },
    };
  }
  const cp = proto.composite as Record<string, unknown> | undefined;
  if (cp) {
    const rawOperator = cp.operator;
    // proto JSON tolerates both int and string forms — accept both on read.
    let operator: CompositeOperator;
    if (typeof rawOperator === 'number') {
      operator = rawOperator as CompositeOperator;
    } else if (typeof rawOperator === 'string') {
      operator = COMPOSITE_OPERATOR_FROM_STRING[rawOperator] ?? CompositeOperator.UNSPECIFIED;
    } else {
      operator = CompositeOperator.UNSPECIFIED;
    }
    const operands = ((cp.operands as Record<string, unknown>[] | undefined) || []).map(
      (op): CompositeOperand => ({
        metricId: (op.metricId as string) || '',
        weight: (op.weight as number | undefined) ?? 0,
      }),
    );
    return { case: 'composite', value: { operands, operator } };
  }
  const wc = proto.windowedCount as Record<string, unknown> | undefined;
  if (wc) {
    return {
      case: 'windowedCount',
      value: {
        eventType: (wc.eventType as string) || '',
        filterSql: (wc.filterSql as string) || '',
        windowHours: (wc.windowHours as number | undefined) ?? 0,
      },
    };
  }
  // Legacy 6 metric types: absent oneof → undefined (NOT { case: undefined }).
  return undefined;
}

const COMPOSITE_OPERATOR_FROM_STRING: Record<string, CompositeOperator> = {
  COMPOSITE_OPERATOR_UNSPECIFIED: CompositeOperator.UNSPECIFIED,
  COMPOSITE_OPERATOR_ADD: CompositeOperator.ADD,
  COMPOSITE_OPERATOR_SUBTRACT: CompositeOperator.SUBTRACT,
  COMPOSITE_OPERATOR_MULTIPLY: CompositeOperator.MULTIPLY,
  COMPOSITE_OPERATOR_DIVIDE: CompositeOperator.DIVIDE,
  COMPOSITE_OPERATOR_WEIGHTED_SUM: CompositeOperator.WEIGHTED_SUM,
};

/** Convert proto JSON metric definition to local MetricDefinition type. */
function adaptMetricDefinition(proto: Record<string, unknown>): MetricDefinition {
  const type = stripEnumPrefix(
    (proto.type as string) || 'MEAN',
    'METRIC_TYPE_',
  ) as MetricType;
  const stakeholderRaw = proto.stakeholder as string | undefined;
  const aggregationRaw = proto.aggregationLevel as string | undefined;

  return {
    metricId: (proto.metricId as string) || '',
    name: (proto.name as string) || '',
    description: (proto.description as string) || '',
    type,
    sourceEventType: (proto.sourceEventType as string) || '',
    numeratorEventType: proto.numeratorEventType as string | undefined,
    denominatorEventType: proto.denominatorEventType as string | undefined,
    percentile: proto.percentile as number | undefined,
    customSql: proto.customSql as string | undefined,
    lowerIsBetter: (proto.lowerIsBetter as boolean) || false,
    surrogateTargetMetricId: proto.surrogateTargetMetricId as string | undefined,
    isQoeMetric: (proto.isQoeMetric as boolean) || false,
    cupedCovariateMetricId: proto.cupedCovariateMetricId as string | undefined,
    minimumDetectableEffect: proto.minimumDetectableEffect as number | undefined,
    stakeholder: stakeholderRaw
      ? (stripEnumPrefix(stakeholderRaw, 'METRIC_STAKEHOLDER_') as MetricStakeholder)
      : undefined,
    aggregationLevel: aggregationRaw
      ? (stripEnumPrefix(aggregationRaw, 'METRIC_AGGREGATION_LEVEL_') as MetricAggregationLevel)
      : undefined,
    typeConfig: adaptMetricTypeConfig(proto),
  };
}

/**
 * Inverse of `adaptMetricDefinition`: convert local MetricDefinition to the
 * proto JSON wire shape the M5 `CreateMetricDefinition` RPC expects.
 *
 * Optional string/number fields with undefined values are omitted from the
 * payload (matches proto3 scalar default semantics — absent == zero value).
 * The discriminated `typeConfig` is expanded to its nested camelCase key.
 */
export function marshalMetricDefinition(metric: MetricDefinition): Record<string, unknown> {
  const out: Record<string, unknown> = {
    metricId: metric.metricId,
    name: metric.name,
    description: metric.description,
    type: `METRIC_TYPE_${metric.type}`,
    sourceEventType: metric.sourceEventType,
    lowerIsBetter: metric.lowerIsBetter,
    isQoeMetric: metric.isQoeMetric,
  };
  if (metric.numeratorEventType !== undefined) out.numeratorEventType = metric.numeratorEventType;
  if (metric.denominatorEventType !== undefined) out.denominatorEventType = metric.denominatorEventType;
  if (metric.percentile !== undefined) out.percentile = metric.percentile;
  if (metric.customSql !== undefined) out.customSql = metric.customSql;
  if (metric.surrogateTargetMetricId !== undefined) out.surrogateTargetMetricId = metric.surrogateTargetMetricId;
  if (metric.cupedCovariateMetricId !== undefined) out.cupedCovariateMetricId = metric.cupedCovariateMetricId;
  if (metric.minimumDetectableEffect !== undefined) out.minimumDetectableEffect = metric.minimumDetectableEffect;
  // ADR-014: stakeholder + aggregation_level enums. Sent as full proto-name
  // strings so the M5 Rust handler's prost JSON decoder routes them correctly.
  if (metric.stakeholder !== undefined) out.stakeholder = `METRIC_STAKEHOLDER_${metric.stakeholder}`;
  if (metric.aggregationLevel !== undefined) out.aggregationLevel = `METRIC_AGGREGATION_LEVEL_${metric.aggregationLevel}`;

  if (metric.typeConfig) {
    switch (metric.typeConfig.case) {
      case 'filteredMean': {
        const v: FilteredMeanConfig = metric.typeConfig.value;
        out.filteredMean = { filterSql: v.filterSql, valueColumn: v.valueColumn };
        break;
      }
      case 'composite': {
        const v: CompositeConfig = metric.typeConfig.value;
        out.composite = {
          operands: v.operands.map((op) => ({ metricId: op.metricId, weight: op.weight })),
          // Emit as integer (proto enum ordinal). String form silently defaulted in BUG-0003 (#552).
          operator: v.operator as number,
        };
        break;
      }
      case 'windowedCount': {
        const v: WindowedCountConfig = metric.typeConfig.value;
        out.windowedCount = {
          eventType: v.eventType,
          filterSql: v.filterSql,
          windowHours: v.windowHours,
        };
        break;
      }
    }
  }
  return out;
}

export interface ListMetricDefinitionsFilters {
  typeFilter?: MetricType;
  pageSize?: number;
  pageToken?: string;
}

export async function listMetricDefinitions(filters?: ListMetricDefinitionsFilters): Promise<ListMetricDefinitionsResponse> {
  const request: Record<string, unknown> = {};
  if (filters?.typeFilter) {
    request.typeFilter = `METRIC_TYPE_${filters.typeFilter}`;
  }
  if (filters?.pageSize) {
    request.pageSize = filters.pageSize;
  }
  if (filters?.pageToken) {
    request.pageToken = filters.pageToken;
  }

  const raw = await callRpc<Record<string, unknown>, { metrics?: Record<string, unknown>[]; nextPageToken?: string }>(
    MGMT_URL, MGMT_SVC, 'ListMetricDefinitions', request,
  );
  return {
    metrics: (raw.metrics || []).map(adaptMetricDefinition),
    nextPageToken: raw.nextPageToken || '',
  };
}

/**
 * Create a new metric definition (ADR-026 Phase 1).
 *
 * Sends `CreateMetricDefinitionRequest{ metric }` to M5 and returns the
 * server-echoed definition (with any backend-applied defaults).
 */
export async function createMetricDefinition(metric: MetricDefinition): Promise<MetricDefinition> {
  // CreateMetricDefinition returns the MetricDefinition DIRECTLY (not wrapped in
  // `{ metric: ... }`). Mirror the defensive fallback pattern used by
  // `createExperiment` above so future servers wrapping the response don't
  // silently empty the returned object.
  const raw = await callRpc<
    { metric: Record<string, unknown> },
    Record<string, unknown> & { metric?: Record<string, unknown> }
  >(MGMT_URL, MGMT_SVC, 'CreateMetricDefinition', { metric: marshalMetricDefinition(metric) }, {
    // Mirror the mutation pattern used by `createExperiment` / `updateExperiment`
    // / `createFlag` etc. — bypass the read cache on the inbound path, and clear
    // it on success so any stale `ListMetricDefinitions` response is dropped.
    skipCache: true,
    clearCacheOnSuccess: true,
  });
  return adaptMetricDefinition(raw.metric ?? raw);
}

/** Fetch one metric definition by id (ADR-026 Phase 1). */
export async function getMetricDefinition(metricId: string): Promise<MetricDefinition> {
  const raw = await callRpc<
    { metricId: string },
    Record<string, unknown> & { metric?: Record<string, unknown> }
  >(MGMT_URL, MGMT_SVC, 'GetMetricDefinition', { metricId });
  return adaptMetricDefinition(raw.metric ?? raw);
}

export interface ListAuditLogFilters {
  experimentId?: string;
  actionFilter?: AuditAction;
  actorEmail?: string;
  pageSize?: number;
  pageToken?: string;
}

/** Adapt proto AuditLogEntry — strip AUDIT_ACTION_ prefix from action enum. */
function adaptAuditLogEntry(proto: Record<string, unknown>): AuditLogEntry {
  return {
    entryId: (proto.entryId as string) || '',
    experimentId: (proto.experimentId as string) || '',
    experimentName: (proto.experimentName as string) || '',
    action: stripEnumPrefix((proto.action as string) || '', 'AUDIT_ACTION_') as AuditAction,
    actorEmail: (proto.actorEmail as string) || '',
    timestamp: (proto.timestamp as string) || '',
    details: (proto.details as string) || '',
    previousValue: proto.previousValue as string | undefined,
    newValue: proto.newValue as string | undefined,
  };
}

export async function listAuditLog(filters?: ListAuditLogFilters): Promise<ListAuditLogResponse> {
  const request: Record<string, unknown> = {};
  if (filters?.experimentId) {
    request.experimentId = filters.experimentId;
  }
  if (filters?.actionFilter) {
    request.actionFilter = `AUDIT_ACTION_${filters.actionFilter}`;
  }
  if (filters?.actorEmail) {
    request.actorEmail = filters.actorEmail;
  }
  if (filters?.pageSize) {
    request.pageSize = filters.pageSize;
  }
  if (filters?.pageToken) {
    request.pageToken = filters.pageToken;
  }

  const raw = await callRpc<Record<string, unknown>, { entries?: Record<string, unknown>[]; nextPageToken?: string }>(
    MGMT_URL, MGMT_SVC, 'ListAuditLog', request,
  );
  return {
    entries: (raw.entries || []).map(adaptAuditLogEntry),
    nextPageToken: raw.nextPageToken || '',
  };
}

// --- Feature Flags (M7) ---

/** Convert proto JSON flag to local Flag type. */
function adaptFlag(proto: Record<string, unknown>): Flag {
  const type = stripEnumPrefix(
    (proto.type as string) || 'BOOLEAN',
    'FLAG_TYPE_',
  ) as FlagType;

  return {
    flagId: (proto.flagId as string) || '',
    name: (proto.name as string) || '',
    description: (proto.description as string) || '',
    type,
    defaultValue: (proto.defaultValue as string) || '',
    enabled: (proto.enabled as boolean) || false,
    rolloutPercentage: (proto.rolloutPercentage as number) || 0,
    variants: ((proto.variants as Record<string, unknown>[]) || []).map((v) => ({
      variantId: (v.variantId as string) || '',
      value: (v.value as string) || '',
      trafficFraction: (v.trafficFraction as number) || 0,
    })),
    targetingRuleId: proto.targetingRuleId as string | undefined,
  };
}

export async function listFlags(pageSize?: number, pageToken?: string): Promise<ListFlagsResponse> {
  const request: Record<string, unknown> = {};
  if (pageSize) request.pageSize = pageSize;
  if (pageToken) request.pageToken = pageToken;

  const raw = await callRpc<Record<string, unknown>, { flags?: Record<string, unknown>[]; nextPageToken?: string }>(
    FLAGS_URL, FLAGS_SVC, 'ListFlags', request,
  );
  return {
    flags: (raw.flags || []).map(adaptFlag),
    nextPageToken: raw.nextPageToken || '',
  };
}

export async function getFlag(flagId: string): Promise<Flag> {
  const raw = await callRpc<{ flagId: string }, Record<string, unknown>>(
    FLAGS_URL, FLAGS_SVC, 'GetFlag', { flagId },
  );
  return adaptFlag(raw);
}

export async function createFlag(flag: Partial<Flag>): Promise<Flag> {
  const raw = await callRpc<{ flag: Partial<Flag> }, Record<string, unknown>>(
    FLAGS_URL, FLAGS_SVC, 'CreateFlag', { flag },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptFlag(raw);
}

export async function updateFlag(flag: Flag): Promise<Flag> {
  const raw = await callRpc<{ flag: Flag }, Record<string, unknown>>(
    FLAGS_URL, FLAGS_SVC, 'UpdateFlag', { flag },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptFlag(raw);
}

export async function promoteToExperiment(
  flagId: string,
  experimentType: string,
  primaryMetricId: string,
  secondaryMetricIds?: string[],
): Promise<Experiment> {
  const raw = await callRpc<
    { flagId: string; experimentType: string; primaryMetricId: string; secondaryMetricIds?: string[] },
    Record<string, unknown>
  >(
    FLAGS_URL, FLAGS_SVC, 'PromoteToExperiment',
    { flagId, experimentType: `EXPERIMENT_TYPE_${experimentType}`, primaryMetricId, secondaryMetricIds },
    { skipCache: true, clearCacheOnSuccess: true },
  );
  return adaptExperiment(raw);
}

// --- Provider Health (ADR-014) ---

export async function getProviderHealth(providerId?: string): Promise<ProviderHealthResult> {
  const request: Record<string, unknown> = {};
  if (providerId) request.providerId = providerId;
  return callRpc<Record<string, unknown>, ProviderHealthResult>(
    METRICS_URL, METRICS_SVC, 'GetProviderHealth', request,
  );
}

// --- AVLM Confidence Sequence (ADR-015) ---

export async function getAvlmResult(experimentId: string, metricId: string): Promise<AvlmResult> {
  return callRpc<{ experimentId: string; metricId: string }, AvlmResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetAvlmResult', { experimentId, metricId },
  );
}

// --- Adaptive Sample Size (ADR-020) ---

export async function getAdaptiveN(experimentId: string): Promise<AdaptiveNResult> {
  return callRpc<{ experimentId: string }, AdaptiveNResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetAdaptiveN', { experimentId },
  );
}

// --- Feedback Loop Analysis ---

export async function getFeedbackLoopAnalysis(experimentId: string): Promise<FeedbackLoopResult> {
  return callRpc<{ experimentId: string }, FeedbackLoopResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetFeedbackLoopAnalysis', { experimentId },
  );
}

// --- Online FDR State (ADR-018) ---

export async function getOnlineFdrState(experimentId: string): Promise<OnlineFdrState> {
  return callRpc<{ experimentId: string }, OnlineFdrState>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetOnlineFdrState', { experimentId },
  );
}

// --- Portfolio Optimization (ADR-019) ---

export async function getPortfolioAllocation(): Promise<PortfolioAllocationResult> {
  return callRpc<Record<string, never>, PortfolioAllocationResult>(
    MGMT_URL, MGMT_SVC, 'GetPortfolioAllocation', {},
  );
}

export async function getPortfolioMetrics(): Promise<PortfolioMetricsResult> {
  return callRpc<Record<string, never>, PortfolioMetricsResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetPortfolioMetrics', {},
  );
}

export async function getOptimalAlpha(experimentId: string): Promise<OptimalAlphaRecommendation> {
  return callRpc<{ experimentId: string }, OptimalAlphaRecommendation>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetOptimalAlpha', { experimentId },
  );
}

// --- Pareto Frontier (ADR-011 / ADR-019) ---

export async function getParetoFrontier(experimentId?: string): Promise<ParetoFrontierResult> {
  const request: Record<string, unknown> = {};
  if (experimentId) request.experimentId = experimentId;
  return callRpc<Record<string, unknown>, ParetoFrontierResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetParetoFrontier', request,
  );
}

// --- Meta-Experiment Results (ADR-013) ---

export async function getMetaExperimentResult(experimentId: string): Promise<MetaExperimentResult> {
  return callRpc<{ experimentId: string }, MetaExperimentResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetMetaExperimentResult', { experimentId },
  );
}

// --- Slate Bandit (ADR-016) ---

export async function getSlateAssignment(
  experimentId: string,
  userId: string,
  candidateItemIds: string[],
  contextFeatures: Record<string, number> = {},
): Promise<SlateAssignmentResponse> {
  return callRpc<{
    experimentId: string;
    userId: string;
    candidateItemIds: string[];
    contextFeatures: Record<string, number>;
  }, SlateAssignmentResponse>(
    ASSIGNMENT_URL, ASSIGNMENT_SVC, 'GetSlateAssignment',
    { experimentId, userId, candidateItemIds, contextFeatures },
    { skipCache: true },
  );
}

export async function getSlateOpe(experimentId: string): Promise<SlateOpeResult> {
  return callRpc<{ experimentId: string }, SlateOpeResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetSlateOpe', { experimentId },
  );
}

export async function getSlateHeatmap(experimentId: string): Promise<SlateHeatmapResult> {
  return callRpc<{ experimentId: string }, SlateHeatmapResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetSlateHeatmap', { experimentId },
  );
}

// --- Switchback Experiments (ADR-022) ---

export async function getSwitchbackResult(experimentId: string): Promise<SwitchbackResult> {
  return callRpc<{ experimentId: string }, SwitchbackResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetSwitchbackResult', { experimentId },
  );
}

// --- Synthetic Control / Quasi-Experiments (ADR-023) ---

export async function getSyntheticControlResult(experimentId: string): Promise<SyntheticControlResult> {
  return callRpc<{ experimentId: string }, SyntheticControlResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetSyntheticControlResult', { experimentId },
  );
}

// --- ADR-026 Phase 2: MetricQL live validation (A9 / B4) ---

/**
 * Wire-format span from ValidateMetricql RPC (A9).
 * Offsets are byte offsets into the MetricQL expression string.
 */
export interface MetricqlDiagnosticSpan {
  startOffset: number;
  endOffset: number;
  line: number;
  column: number;
}

/**
 * Wire-format diagnostic from ValidateMetricql RPC.
 * severity: 0 = unspecified, 1 = error, 2 = warning.
 */
export interface MetricqlDiagnosticEntry {
  severity: number;
  message: string;
  span: MetricqlDiagnosticSpan | null;
}

/** Wire-format response from ValidateMetricql RPC (A9, M5 ExperimentManagementService). */
export interface ValidateMetricqlRpcResponse {
  diagnostics: MetricqlDiagnosticEntry[];
  referencedMetricIds: string[];
}

/**
 * Call the A9 ValidateMetricql RPC (M5 ExperimentManagementService).
 *
 * - skipCache: true — every keystroke yields a unique expression, cache is useless.
 * - retries: 0 — abort-on-retype makes retries counterproductive.
 * - signal: passed through for AbortController cancel-in-flight from the linter.
 *
 * Returns null when the request was aborted (cancel-in-flight or timeout); the
 * linter layer treats null as "no update to markers right now".
 */
export async function validateMetricql(
  args: { experimentId: string; metricqlExpression: string },
  options: { signal: AbortSignal },
): Promise<ValidateMetricqlRpcResponse | null> {
  try {
    const result = await callRpcWithSignal<
      { experimentId: string; metricqlExpression: string },
      ValidateMetricqlRpcResponse
    >(
      MGMT_URL,
      MGMT_SVC,
      'ValidateMetricql',
      { experimentId: args.experimentId, metricqlExpression: args.metricqlExpression },
      { skipCache: true, retries: 0 },
      options.signal,
    );
    return result;
  } catch (err) {
    // AbortError covers both cancel-in-flight (user typed) and timeout.
    if ((err as DOMException).name === 'AbortError' || options.signal.aborted) {
      return null;
    }
    throw err;
  }
}

// --- ADR-026 Phase 2: MetricQL compiled-SQL preview (B5 / C2) ---

/**
 * Wire-format response from PreviewMetricDefinition RPC (C2, M5 proxies to M3).
 *
 * compiledSql: the Spark SQL generated by M3's MetricQL compiler.
 * diagnostics: server-side parse/semantic diagnostics (mirrors ValidateMetricql format).
 *   Non-empty when the expression compiles on M5 but M3 finds additional issues.
 */
export interface PreviewMetricDefinitionResponse {
  compiledSql: string;
  diagnostics: MetricqlDiagnosticEntry[];
}

/**
 * Call the C2 PreviewMetricDefinition RPC (M5 ExperimentManagementService).
 *
 * - skipCache: true — expression-specific, caching is not useful.
 * - retries: 0 — AbortController cancel-on-unmount makes retries counterproductive.
 * - signal: passed through for AbortController cancel-in-flight from the preview pane.
 *
 * Returns null when the request was aborted (cancel-on-unmount or timeout); the
 * preview pane treats null as a no-op (state is already handled by the caller).
 */
export async function previewMetricDefinition(
  args: { experimentId: string; metricqlExpression: string },
  options: { signal: AbortSignal },
): Promise<PreviewMetricDefinitionResponse | null> {
  try {
    const result = await callRpcWithSignal<
      { experimentId: string; metricqlExpression: string },
      PreviewMetricDefinitionResponse
    >(
      MGMT_URL,
      MGMT_SVC,
      'PreviewMetricDefinition',
      { experimentId: args.experimentId, metricqlExpression: args.metricqlExpression },
      { skipCache: true, retries: 0 },
      options.signal,
    );
    return result;
  } catch (err) {
    // AbortError covers both cancel-on-unmount and the 5s client timeout.
    if ((err as DOMException).name === 'AbortError' || options.signal.aborted) {
      return null;
    }
    throw err;
  }
}
