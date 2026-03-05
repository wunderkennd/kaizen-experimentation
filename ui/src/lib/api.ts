import type { Experiment, ListExperimentsResponse } from './types';

const API_URL = process.env.API_URL || process.env.NEXT_PUBLIC_API_URL || 'http://localhost:50055';

export async function listExperiments(): Promise<ListExperimentsResponse> {
  const res = await fetch(`${API_URL}/api/experiments`);
  if (!res.ok) {
    throw new Error(`Failed to list experiments: ${res.status}`);
  }
  return res.json();
}

export async function getExperiment(id: string): Promise<Experiment> {
  const res = await fetch(`${API_URL}/api/experiments/${id}`);
  if (!res.ok) {
    throw new Error(`Failed to get experiment: ${res.status}`);
  }
  return res.json();
}

export async function updateExperiment(experiment: Experiment): Promise<Experiment> {
  const res = await fetch(`${API_URL}/api/experiments/${experiment.experimentId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(experiment),
  });
  if (!res.ok) {
    throw new Error(`Failed to update experiment: ${res.status}`);
  }
  return res.json();
}

export async function startExperiment(id: string): Promise<Experiment> {
  const res = await fetch(`${API_URL}/api/experiments/${id}/start`, {
    method: 'POST',
  });
  if (!res.ok) {
    throw new Error(`Failed to start experiment: ${res.status}`);
  }
  return res.json();
}

export async function concludeExperiment(id: string): Promise<Experiment> {
  const res = await fetch(`${API_URL}/api/experiments/${id}/conclude`, {
    method: 'POST',
  });
  if (!res.ok) {
    throw new Error(`Failed to conclude experiment: ${res.status}`);
  }
  return res.json();
}

export async function archiveExperiment(id: string): Promise<Experiment> {
  const res = await fetch(`${API_URL}/api/experiments/${id}/archive`, {
    method: 'POST',
  });
  if (!res.ok) {
    throw new Error(`Failed to archive experiment: ${res.status}`);
  }
  return res.json();
}
