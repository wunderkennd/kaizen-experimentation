'use client';

import { useEffect, useState, useCallback } from 'react';
import { useParams, useRouter } from 'next/navigation';
import Link from 'next/link';
import type { Flag, FlagType } from '@/lib/types';
import { getFlag, updateFlag } from '@/lib/api';
import { RetryableError } from '@/components/retryable-error';
import { AuthProvider, useAuth } from '@/lib/auth-context';
import { NavHeader } from '@/components/nav-header';

const FLAG_TYPES: FlagType[] = ['BOOLEAN', 'STRING', 'NUMERIC', 'JSON'];

function EditFlagContent() {
  const params = useParams();
  const router = useRouter();
  const { canAtLeast } = useAuth();
  const canEdit = canAtLeast('experimenter');
  const flagId = params.id as string;

  const [flag, setFlag] = useState<Flag | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [type, setType] = useState<FlagType>('BOOLEAN');
  const [defaultValue, setDefaultValue] = useState('');
  const [enabled, setEnabled] = useState(false);
  const [rolloutPct, setRolloutPct] = useState(0);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const fetchData = useCallback(() => {
    setLoading(true);
    setError(null);
    getFlag(flagId)
      .then((data) => {
        setFlag(data);
        setName(data.name);
        setDescription(data.description);
        setType(data.type);
        setDefaultValue(data.defaultValue);
        setEnabled(data.enabled);
        setRolloutPct(Math.round(data.rolloutPercentage * 100));
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, [flagId]);

  useEffect(() => { fetchData(); }, [fetchData]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!flag || !name.trim()) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      await updateFlag({
        ...flag,
        name: name.trim(),
        description,
        type,
        defaultValue,
        enabled,
        rolloutPercentage: rolloutPct / 100,
      });
      router.push(`/flags/${flagId}`);
    } catch (err) {
      setSubmitError(err instanceof Error ? err.message : 'Failed to update flag');
    } finally {
      setSubmitting(false);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12" role="status" aria-label="Loading">
        <div className="h-8 w-8 animate-spin rounded-full border-4 border-gray-300 border-t-indigo-600" />
        <span className="sr-only">Loading</span>
      </div>
    );
  }

  if (error) {
    return <RetryableError message={error} onRetry={fetchData} context="flag" />;
  }

  if (!flag) return null;

  if (!canEdit) {
    return (
      <div className="py-12 text-center">
        <p className="text-sm text-gray-500">You need experimenter permissions to edit flags.</p>
      </div>
    );
  }

  return (
    <div>
      <div className="mb-2">
        <Link href={`/flags/${flagId}`} className="text-sm text-indigo-600 hover:text-indigo-800" data-testid="back-link">
          &larr; Back to {flag.name}
        </Link>
      </div>

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Edit Flag</h1>

      <form onSubmit={handleSubmit} className="max-w-lg rounded-lg border border-gray-200 bg-white p-6 shadow-sm" data-testid="edit-flag-form">
        <div className="mb-4">
          <label htmlFor="flag-name" className="mb-1 block text-sm font-medium text-gray-700">Name *</label>
          <input
            id="flag-name"
            type="text"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="edit-flag-name"
          />
        </div>

        <div className="mb-4">
          <label htmlFor="flag-desc" className="mb-1 block text-sm font-medium text-gray-700">Description</label>
          <textarea
            id="flag-desc"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            rows={2}
            className="w-full rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="edit-flag-desc"
          />
        </div>

        <div className="mb-4">
          <label htmlFor="flag-type" className="mb-1 block text-sm font-medium text-gray-700">Type</label>
          <select
            id="flag-type"
            value={type}
            onChange={(e) => setType(e.target.value as FlagType)}
            className="w-full rounded-md border border-gray-300 px-3 py-2 text-sm"
            data-testid="edit-flag-type"
          >
            {FLAG_TYPES.map((t) => (
              <option key={t} value={t}>{t}</option>
            ))}
          </select>
        </div>

        <div className="mb-4">
          <label htmlFor="flag-default" className="mb-1 block text-sm font-medium text-gray-700">Default Value</label>
          <input
            id="flag-default"
            type="text"
            value={defaultValue}
            onChange={(e) => setDefaultValue(e.target.value)}
            className="w-full rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="edit-flag-default"
          />
        </div>

        <div className="mb-4 flex items-center gap-2">
          <input
            id="flag-enabled"
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
            data-testid="edit-flag-enabled"
          />
          <label htmlFor="flag-enabled" className="text-sm font-medium text-gray-700">Enabled</label>
        </div>

        <div className="mb-6">
          <label htmlFor="flag-rollout" className="mb-1 block text-sm font-medium text-gray-700">
            Rollout Percentage: {rolloutPct}%
          </label>
          <input
            id="flag-rollout"
            type="range"
            min={0}
            max={100}
            value={rolloutPct}
            onChange={(e) => setRolloutPct(Number(e.target.value))}
            className="w-full"
            data-testid="edit-flag-rollout"
          />
        </div>

        {submitError && (
          <p className="mb-4 text-sm text-red-600" data-testid="edit-error">{submitError}</p>
        )}

        <div className="flex gap-3">
          <button
            type="submit"
            disabled={submitting || !name.trim()}
            className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            data-testid="edit-submit"
          >
            {submitting ? 'Saving...' : 'Save Changes'}
          </button>
          <Link
            href={`/flags/${flagId}`}
            className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
          >
            Cancel
          </Link>
        </div>
      </form>
    </div>
  );
}

export default function EditFlagPage() {
  return (
    <AuthProvider>
      <div className="min-h-screen bg-gray-50">
        <NavHeader />
        <main className="mx-auto max-w-7xl px-4 py-8 sm:px-6 lg:px-8">
          <EditFlagContent />
        </main>
      </div>
    </AuthProvider>
  );
}
