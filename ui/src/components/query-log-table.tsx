'use client';

import { useState } from 'react';
import dynamic from 'next/dynamic';
import type { QueryLogEntry } from '@/lib/types';
import type { ExportPhase } from '@/lib/export-notebook';
import { CopyButton } from '@/components/copy-button';

const SqlHighlighter = dynamic(
  () => import('@/components/sql-highlighter').then(m => ({ default: m.SqlHighlighter })),
  { ssr: false, loading: () => <pre className="animate-pulse mt-2 rounded bg-gray-50 p-3 font-mono text-xs">Loading...</pre> },
);

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

const EXPORT_PHASE_LABELS: Record<ExportPhase, string> = {
  fetching: 'Fetching data…',
  decoding: 'Processing…',
  downloading: 'Exporting…',
};

interface QueryLogTableProps {
  entries: QueryLogEntry[];
  onExport: () => void;
  exporting: boolean;
  exportPhase?: ExportPhase | null;
}

export function QueryLogTable({ entries, onExport, exporting, exportPhase }: QueryLogTableProps) {
  const [expandedIndex, setExpandedIndex] = useState<number | null>(null);

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <p className="text-sm text-gray-500">{entries.length} queries</p>
        <button
          onClick={onExport}
          disabled={exporting}
          className="inline-flex items-center gap-2 rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2 disabled:opacity-50"
        >
          {exporting && (
            <svg
              className="h-4 w-4 animate-spin text-gray-600"
              xmlns="http://www.w3.org/2000/svg"
              fill="none"
              viewBox="0 0 24 24"
              aria-hidden="true"
              data-testid="export-spinner"
            >
              <circle
                className="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                strokeWidth="4"
              />
              <path
                className="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
              />
            </svg>
          )}
          {exporting ? (exportPhase ? EXPORT_PHASE_LABELS[exportPhase] : 'Exporting…') : 'Export Notebook'}
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
              <tr key={`${entry.metricId}-${i}`} className="group hover:bg-gray-50 focus-within:bg-gray-50">
                <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                  <div className="flex items-center gap-2">
                    <span>{entry.metricId}</span>
                    <CopyButton
                      value={entry.metricId}
                      label={`Copy metric ID ${entry.metricId}`}
                      successMessage="Metric ID copied to clipboard"
                      className="opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100"
                    />
                  </div>
                </td>
                <td className="px-4 py-3 text-sm text-gray-600">
                  <button
                    onClick={() => setExpandedIndex(expandedIndex === i ? null : i)}
                    aria-expanded={expandedIndex === i}
                    aria-label={`Toggle SQL preview for ${entry.metricId}`}
                    className="max-w-md truncate text-left font-mono text-xs text-gray-600 hover:text-indigo-600 rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500 focus-visible:ring-offset-2"
                  >
                    {entry.sqlText.slice(0, 100)}{entry.sqlText.length > 100 ? '…' : ''}
                  </button>
                  {expandedIndex === i && (
                    <SqlHighlighter sql={entry.sqlText} />
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
