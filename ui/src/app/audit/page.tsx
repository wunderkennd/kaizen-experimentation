'use client';

import { useEffect, useState, useCallback } from 'react';
import type { AuditLogEntry, AuditAction } from '@/lib/types';
import { listAuditLog } from '@/lib/api';
import { AuditLogTable } from '@/components/audit-log-table';
import { AuditFilters } from '@/components/audit-filters';
import { RetryableError } from '@/components/retryable-error';
import { Breadcrumb } from '@/components/breadcrumb';

export default function AuditLogPage() {
  const [entries, setEntries] = useState<AuditLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [nextPageToken, setNextPageToken] = useState('');
  const [loadingMore, setLoadingMore] = useState(false);

  // Filter state
  const [experimentQuery, setExperimentQuery] = useState('');
  const [actionFilter, setActionFilter] = useState<AuditAction | ''>('');
  const [actorQuery, setActorQuery] = useState('');

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    listAuditLog()
      .then((data) => {
        setEntries(data.entries);
        setNextPageToken(data.nextPageToken);
      })
      .catch((err) => {
        setError(err.message);
      })
      .finally(() => {
        setLoading(false);
      });
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const loadMore = useCallback(() => {
    if (!nextPageToken || loadingMore) return;
    setLoadingMore(true);
    listAuditLog({ pageSize: 10, pageToken: nextPageToken })
      .then((data) => {
        setEntries((prev) => [...prev, ...data.entries]);
        setNextPageToken(data.nextPageToken);
      })
      .catch((err) => {
        setError(err.message);
      })
      .finally(() => {
        setLoadingMore(false);
      });
  }, [nextPageToken, loadingMore]);

  const hasActiveFilters = experimentQuery !== '' || actionFilter !== '' || actorQuery !== '';

  const clearFilters = () => {
    setExperimentQuery('');
    setActionFilter('');
    setActorQuery('');
  };

  // Client-side filtering
  const filtered = entries.filter((entry) => {
    if (experimentQuery && !entry.experimentName.toLowerCase().includes(experimentQuery.toLowerCase())) {
      return false;
    }
    if (actionFilter && entry.action !== actionFilter) {
      return false;
    }
    if (actorQuery && !entry.actorEmail.toLowerCase().includes(actorQuery.toLowerCase())) {
      return false;
    }
    return true;
  });

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return <RetryableError message={error} onRetry={fetchData} context="audit log" />;
  }

  if (entries.length === 0) {
    return (
      <div>
        <h1 className="mb-6 text-2xl font-bold text-gray-900">Audit Log</h1>
        <div className="py-12 text-center">
          <p className="text-sm text-gray-500">No audit log entries found.</p>
        </div>
      </div>
    );
  }

  return (
    <div>
      <Breadcrumb items={[
        { label: 'Experiments', href: '/' },
        { label: 'Audit Log' },
      ]} />

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Audit Log</h1>

      <AuditFilters
        experimentQuery={experimentQuery}
        onExperimentQueryChange={setExperimentQuery}
        actionFilter={actionFilter}
        onActionFilterChange={setActionFilter}
        actorQuery={actorQuery}
        onActorQueryChange={setActorQuery}
        totalCount={entries.length}
        filteredCount={filtered.length}
        onClear={clearFilters}
        hasActiveFilters={hasActiveFilters}
      />

      {filtered.length === 0 ? (
        <div className="py-12 text-center" data-testid="no-filter-matches">
          <p className="text-sm text-gray-500">No audit log entries match your filters.</p>
          <button
            onClick={clearFilters}
            className="mt-2 text-sm text-indigo-600 hover:text-indigo-800"
          >
            Clear filters
          </button>
        </div>
      ) : (
        <>
          <AuditLogTable entries={filtered} />
          {nextPageToken && (
            <div className="mt-4 flex justify-center">
              <button
                onClick={loadMore}
                disabled={loadingMore}
                className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed"
                data-testid="load-more-button"
              >
                {loadingMore ? 'Loading...' : 'Load More'}
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
