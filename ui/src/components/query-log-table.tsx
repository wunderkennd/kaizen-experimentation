'use client';

import { useState } from 'react';
import type { QueryLogEntry } from '@/lib/types';

function formatDuration(ms: number): string {
  if (ms >= 1000) {
    return `${(ms / 1000).toFixed(1)}s`;
  }
  return `${ms}ms`;
}

function formatRowCount(count: number): string {
  return count.toLocaleString('en-US');
}

function formatRelativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const minutes = Math.floor(diff / 60000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

interface QueryLogTableProps {
  entries: QueryLogEntry[];
  onExport: () => void;
  exporting: boolean;
}

export function QueryLogTable({ entries, onExport, exporting }: QueryLogTableProps) {
  const [expandedIndex, setExpandedIndex] = useState<number | null>(null);

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <p className="text-sm text-gray-500">{entries.length} queries</p>
        <button
          onClick={onExport}
          disabled={exporting}
          className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
        >
          {exporting ? 'Exporting…' : 'Export Notebook'}
        </button>
      </div>
      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Metric
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                SQL Preview
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Rows
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Duration
              </th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                Computed
              </th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {entries.map((entry, i) => (
              <tr key={`${entry.metricId}-${i}`} className="group">
                <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                  {entry.metricId}
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">
                  <button
                    onClick={() => setExpandedIndex(expandedIndex === i ? null : i)}
                    aria-expanded={expandedIndex === i}
                    aria-label={`Toggle SQL preview for ${entry.metricId}`}
                    className="max-w-md truncate text-left font-mono text-xs text-gray-600 hover:text-indigo-600"
                  >
                    {entry.sqlText.slice(0, 100)}{entry.sqlText.length > 100 ? '…' : ''}
                  </button>
                  {expandedIndex === i && (
                    <pre className="mt-2 overflow-x-auto rounded bg-gray-50 p-3 font-mono text-xs text-gray-800">
                      {entry.sqlText}
                    </pre>
                  )}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {formatRowCount(entry.rowCount)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                  {formatDuration(entry.durationMs)}
                </td>
                <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-500">
                  {entry.computedAt ? formatRelativeTime(entry.computedAt) : '—'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
