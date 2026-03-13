'use client';

import { useState, useEffect, useRef, useCallback } from 'react';

export interface ServiceHealth {
  name: string;
  url: string;
  healthy: boolean;
  latencyMs: number | null;
  error?: string;
}

export interface HealthStatus {
  services: ServiceHealth[];
  allHealthy: boolean;
  checkedAt: string;
}

const HEALTH_TIMEOUT_MS = 3_000;

const MGMT_URL =
  process.env.NEXT_PUBLIC_MANAGEMENT_URL || '/api/rpc/management';
const MGMT_SVC =
  'experimentation.management.v1.ExperimentManagementService';

interface ServiceConfig {
  name: string;
  url: string;
  service: string;
  method: string;
}

const SERVICES: ServiceConfig[] = [
  { name: 'Management', url: MGMT_URL, service: MGMT_SVC, method: 'ListExperiments' },
];

async function pingService(cfg: ServiceConfig): Promise<ServiceHealth> {
  const start = performance.now();
  let timer: ReturnType<typeof setTimeout>;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new DOMException('Timeout', 'AbortError')),
      HEALTH_TIMEOUT_MS,
    );
  });

  try {
    const res = await Promise.race([
      fetch(`${cfg.url}/${cfg.service}/${cfg.method}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ pageSize: 1 }),
      }),
      timeoutPromise,
    ]);
    clearTimeout(timer!);
    const latencyMs = Math.round(performance.now() - start);
    // Any HTTP response (even 4xx for auth) means the service is reachable.
    // Only 5xx indicates unhealthy.
    const healthy = res.status < 500;
    return { name: cfg.name, url: cfg.url, healthy, latencyMs };
  } catch (err) {
    clearTimeout(timer!);
    const msg =
      err instanceof DOMException && err.name === 'AbortError'
        ? 'Timeout'
        : err instanceof Error
          ? err.message
          : 'Unknown error';
    return {
      name: cfg.name,
      url: cfg.url,
      healthy: false,
      latencyMs: null,
      error: msg,
    };
  }
}

export async function checkHealth(): Promise<HealthStatus> {
  const results = await Promise.all(SERVICES.map(pingService));
  return {
    services: results,
    allHealthy: results.every((s) => s.healthy),
    checkedAt: new Date().toISOString(),
  };
}

export function useHealthCheck(intervalMs = 30_000) {
  const [status, setStatus] = useState<HealthStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const isMockMode =
    process.env.NEXT_PUBLIC_MOCK_API === 'true';

  const runCheck = useCallback(async () => {
    if (isMockMode) return;
    setChecking(true);
    try {
      const result = await checkHealth();
      setStatus(result);
    } finally {
      setChecking(false);
    }
  }, [isMockMode]);

  useEffect(() => {
    if (isMockMode) return;

    runCheck();
    intervalRef.current = setInterval(runCheck, intervalMs);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
      }
    };
  }, [isMockMode, intervalMs, runCheck]);

  return { status, checking, isMockMode };
}
