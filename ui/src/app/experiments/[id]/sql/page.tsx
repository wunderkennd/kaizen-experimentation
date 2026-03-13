'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import type { QueryLogEntry } from '@/lib/types';
import { getQueryLog } from '@/lib/api';
import { downloadNotebook, type ExportPhase } from '@/lib/export-notebook';
import { QueryLogTable } from '@/components/query-log-table';
import { RetryableError } from '@/components/retryable-error';

export default function SqlPage() {
  const params = useParams<{ id: string }>();
  const [entries, setEntries] = useState<QueryLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportPhase, setExportPhase] = useState<ExportPhase | null>(null);

  const fetchData = useCallback(() => {
    if (!params.id) return;
    setLoading(true);
    setError(null);
    getQueryLog(params.id)
      .then(setEntries)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id]);

  useEffect(() => { fetchData(); }, [fetchData]);

  const handleExport = useCallback(async () => {
    if (!params.id) return;
    setExporting(true);
    setExportPhase(null);
    try {
      await downloadNotebook(params.id, {
        onProgress: (phase) => setExportPhase(phase),
      });
    } catch (err) {
      if (err instanceof DOMException && err.name === 'AbortError') {
        setError('Export timed out — notebook may be too large');
      } else {
        setError('Failed to export notebook');
      }
    } finally {
      setExporting(false);
      setExportPhase(null);
    }
  }, [params.id]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return (
      <div>
        <nav className="mb-4 text-sm text-gray-500">
          <Link href="/" className="hover:text-indigo-600">Experiments</Link>
          <span className="mx-2">/</span>
          <Link href={`/experiments/${params.id}`} className="hover:text-indigo-600">Detail</Link>
          <span className="mx-2">/</span>
          <span className="text-gray-900">SQL</span>
        </nav>
        <RetryableError message={error} onRetry={fetchData} context="query log" />
      </div>
    );
  }

  return (
    <div>
      <nav className="mb-4 text-sm text-gray-500">
        <Link href="/" className="hover:text-indigo-600">Experiments</Link>
        <span className="mx-2">/</span>
        <Link href={`/experiments/${params.id}`} className="hover:text-indigo-600">Detail</Link>
        <span className="mx-2">/</span>
        <span className="text-gray-900">SQL</span>
      </nav>

      <h1 className="mb-4 text-2xl font-bold text-gray-900">Query Log</h1>

      {entries.length === 0 ? (
        <div className="py-12 text-center">
          <p className="text-sm text-gray-500">No query log entries found for this experiment.</p>
        </div>
      ) : (
        <QueryLogTable entries={entries} onExport={handleExport} exporting={exporting} exportPhase={exportPhase} />
      )}
    </div>
  );
}
