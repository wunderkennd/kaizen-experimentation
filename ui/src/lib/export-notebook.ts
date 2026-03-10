import { exportNotebook } from './api';

export type ExportPhase = 'fetching' | 'decoding' | 'downloading';

interface DownloadNotebookOptions {
  onProgress?: (phase: ExportPhase) => void;
  timeoutMs?: number;
}

/** Threshold above which we offload base64 decode to a Web Worker. */
const WORKER_THRESHOLD_BYTES = 10_000;

/** Decode base64 string synchronously (fallback for small payloads / no Worker). */
function decodeSyncFallback(base64: string): ArrayBuffer {
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }
  return bytes.buffer;
}

/** Decode base64 using a Web Worker (non-blocking). */
function decodeWithWorker(base64: string): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    try {
      const worker = new Worker(
        new URL('../workers/decode-base64.ts', import.meta.url),
        { type: 'module' },
      );
      worker.onmessage = (event: MessageEvent<ArrayBuffer>) => {
        resolve(event.data);
        worker.terminate();
      };
      worker.onerror = (err) => {
        reject(new Error(err.message || 'Worker decode failed'));
        worker.terminate();
      };
      worker.postMessage(base64);
    } catch {
      // Worker not available (e.g., jsdom) — fall back to sync
      resolve(decodeSyncFallback(base64));
    }
  });
}

/**
 * Fetch, decode, and trigger download of a Jupyter notebook export.
 * Reports progress phases: 'fetching' → 'decoding' → 'downloading'.
 */
export async function downloadNotebook(
  experimentId: string,
  options: DownloadNotebookOptions = {},
): Promise<void> {
  const { onProgress, timeoutMs = 30_000 } = options;

  // Phase 1: Fetch
  onProgress?.('fetching');
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  let result: { content: string; filename: string };
  try {
    result = await exportNotebook(experimentId);
  } finally {
    clearTimeout(timer);
  }

  // Phase 2: Decode
  onProgress?.('decoding');
  let buffer: ArrayBuffer;
  if (result.content.length > WORKER_THRESHOLD_BYTES && typeof Worker !== 'undefined') {
    buffer = await decodeWithWorker(result.content);
  } else {
    buffer = decodeSyncFallback(result.content);
  }

  // Phase 3: Download
  onProgress?.('downloading');
  const blob = new Blob([buffer], { type: 'application/x-ipynb+json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = result.filename;
  a.click();
  URL.revokeObjectURL(url);
}
