'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams } from 'next/navigation';
import Link from 'next/link';
import type { QueryLogEntry } from '@/lib/types';
import { getQueryLog, exportNotebook } from '@/lib/api';
import { QueryLogTable } from '@/components/query-log-table';

export default function SqlPage() {
  const params = useParams<{ id: string }>();
  const [entries, setEntries] = useState<QueryLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);

  useEffect(() => {
    if (!params.id) return;

    getQueryLog(params.id)
      .then(setEntries)
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id]);

  const handleExport = useCallback(async () => {
    if (!params.id) return;
    setExporting(true);
    try {
      const result = await exportNotebook(params.id);
      const blob = new Blob([atob(result.content)], { type: 'application/x-ipynb+json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = result.filename;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      setError('Failed to export notebook');
    } finally {
      setExporting(false);
    }
  }, [params.id]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
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
        <div className="rounded-md bg-red-50 p-4">
          <p className="text-sm text-red-700">{error}</p>
        </div>
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
        <QueryLogTable entries={entries} onExport={handleExport} exporting={exporting} />
      )}
    </div>
  );
}
