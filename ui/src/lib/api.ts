import type { AnalysisResult, Experiment, ListExperimentsResponse, QueryLogEntry } from './types';
import type { ExperimentState, ExperimentType } from './types';

const MGMT_URL = process.env.NEXT_PUBLIC_MANAGEMENT_URL || 'http://localhost:50055';
const MGMT_SVC = 'experimentation.management.v1.ExperimentManagementService';

const METRICS_URL = process.env.NEXT_PUBLIC_METRICS_URL || 'http://localhost:50054';
const METRICS_SVC = 'experimentation.metrics.v1.MetricComputationService';

const ANALYSIS_URL = process.env.NEXT_PUBLIC_ANALYSIS_URL || 'http://localhost:50053';
const ANALYSIS_SVC = 'experimentation.analysis.v1.AnalysisService';

async function callRpc<Req, Res>(baseUrl: string, service: string, method: string, request: Req): Promise<Res> {
  const res = await fetch(`${baseUrl}/${service}/${method}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!res.ok) throw new Error(`RPC ${method} failed: ${res.status}`);
  return res.json();
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

export async function listExperiments(): Promise<ListExperimentsResponse> {
  const raw = await callRpc<object, { experiments?: Record<string, unknown>[]; nextPageToken?: string }>(
    MGMT_URL, MGMT_SVC, 'ListExperiments', {},
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
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function startExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'StartExperiment', { experimentId: id },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function concludeExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'ConcludeExperiment', { experimentId: id },
  );
  return adaptExperiment(raw.experiment || raw as Record<string, unknown>);
}

export async function archiveExperiment(id: string): Promise<Experiment> {
  const raw = await callRpc<{ experimentId: string }, { experiment?: Record<string, unknown> }>(
    MGMT_URL, MGMT_SVC, 'ArchiveExperiment', { experimentId: id },
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
  );
  return raw;
}

export async function getAnalysisResult(experimentId: string): Promise<AnalysisResult> {
  return callRpc<{ experimentId: string }, AnalysisResult>(
    ANALYSIS_URL, ANALYSIS_SVC, 'GetAnalysisResult', { experimentId },
  );
}
