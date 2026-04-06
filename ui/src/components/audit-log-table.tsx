'use client';

import { useState } from 'react';
import Link from 'next/link';
import type { AuditLogEntry } from '@/lib/types';
import { AuditActionBadge } from '@/components/audit-action-badge';
import { CopyButton } from '@/components/copy-button';

function formatTimestamp(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

interface AuditLogTableProps {
  entries: AuditLogEntry[];
}

export function AuditLogTable({ entries }: AuditLogTableProps) {
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const toggleExpand = (entryId: string) => {
    setExpandedId((prev) => (prev === entryId ? null : entryId));
  };

  return (
    <div className="overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm">
      <table className="min-w-full divide-y divide-gray-200">
        <thead className="bg-gray-50">
          <tr>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Timestamp
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Experiment
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Action
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Actor
            </th>
            <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
              Details
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {entries.map((entry) => {
            const isExpanded = expandedId === entry.entryId;
            return (
              <tr
                key={entry.entryId}
                className="cursor-pointer hover:bg-gray-50"
                onClick={() => toggleExpand(entry.entryId)}
                data-testid={`audit-row-${entry.entryId}`}
              >
                <td className="px-4 py-3 text-sm text-gray-600 whitespace-nowrap align-top">
                  {formatTimestamp(entry.timestamp)}
                </td>
                <td className="px-4 py-3 text-sm align-top">
                  <Link
                    href={`/experiments/${entry.experimentId}`}
                    className="text-indigo-600 hover:text-indigo-800 hover:underline"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {entry.experimentName}
                  </Link>
                </td>
                <td className="px-4 py-3 text-sm align-top">
                  <AuditActionBadge action={entry.action} />
                </td>
                <td className="px-4 py-3 text-sm text-gray-600 align-top">
                  {entry.actorEmail}
                </td>
                <td className="px-4 py-3 text-sm text-gray-700 align-top">
                  <div>{entry.details}</div>
                  <span className="text-xs text-gray-400">
                    {isExpanded ? '(click to collapse)' : '(click to expand details)'}
                  </span>
                  {isExpanded && (
                    <div className="mt-2 rounded-md bg-gray-50 p-3 text-xs" data-testid={`audit-detail-${entry.entryId}`}>
                      <div className="mb-2">
                        <div className="flex items-center justify-between gap-2">
                          <span className="font-semibold text-gray-500">Experiment ID: </span>
                          <CopyButton
                            value={entry.experimentId}
                            label="Copy experiment ID"
                            successMessage="Experiment ID copied"
                            className="h-4 w-4"
                          />
                        </div>
                        <code className="mt-1 block rounded bg-gray-100 px-1.5 py-1 font-mono text-[10px] text-gray-600 break-all border border-gray-200">
                          {entry.experimentId}
                        </code>
                      </div>
                      {entry.previousValue && (
                        <div className="mb-2">
                          <div className="flex items-center justify-between gap-2">
                            <span className="font-semibold text-red-600">Previous: </span>
                            <CopyButton
                              value={entry.previousValue}
                              label="Copy previous value"
                              successMessage="Previous value copied"
                              className="h-4 w-4"
                            />
                          </div>
                          <code className="mt-1 block break-all rounded bg-red-50/50 p-1">{entry.previousValue}</code>
                        </div>
                      )}
                      {entry.newValue && (
                        <div>
                          <div className="flex items-center justify-between gap-2">
                            <span className="font-semibold text-green-600">New: </span>
                            <CopyButton
                              value={entry.newValue}
                              label="Copy new value"
                              successMessage="New value copied"
                              className="h-4 w-4"
                            />
                          </div>
                          <code className="mt-1 block break-all rounded bg-green-50/50 p-1">{entry.newValue}</code>
                        </div>
                      )}
                    </div>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
