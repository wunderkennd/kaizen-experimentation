import type {
  AnalysisResult, CreateExperimentRequest, Experiment, ListExperimentsResponse,
  QueryLogEntry, NoveltyAnalysisResult, InterferenceAnalysisResult, InterleavingAnalysisResult,
  BanditDashboardResult, CumulativeHoldoutResult, GuardrailStatusResult, QoeDashboardResult,
  GstTrajectoryResult, CateAnalysisResult, Layer, LayerAllocation,
} from './types';
import type { ExperimentState, ExperimentType } from './types';

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

  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (_authEmail) headers['X-User-Email'] = _authEmail;
  if (_authRole) headers['X-User-Role'] = _authRole;

  const res = await fetch(`${baseUrl}/${service}/${method}`, {
    method: 'POST',
    headers,
    body: JSON.stringify(request),
  });
  if (!res.ok) {
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

export async function getAnalysisResult(experimentId: string): Promise<AnalysisResult> {
  return callRpc<{ experimentId: string }, AnalysisResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetAnalysisResult', { experimentId },
  );
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
