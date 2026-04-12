'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import type { FlagType } from '@/lib/types';
import { createFlag } from '@/lib/api';
import { useAuth } from '@/lib/auth-context';

const FLAG_TYPES: FlagType[] = ['BOOLEAN', 'STRING', 'NUMERIC', 'JSON'];

function CreateFlagContent() {
  const router = useRouter();
  const { canAtLeast } = useAuth();
  const canCreate = canAtLeast('experimenter');

  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [type, setType] = useState<FlagType>('BOOLEAN');
  const [defaultValue, setDefaultValue] = useState('false');
  const [enabled, setEnabled] = useState(false);
  const [rolloutPct, setRolloutPct] = useState(0);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    setSubmitting(true);
    setError(null);
    try {
      const flag = await createFlag({
        name: name.trim(),
        description,
        type,
        defaultValue,
        enabled,
        rolloutPercentage: rolloutPct / 100,
      });
      router.push(`/flags/${flag.flagId}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create flag');
    } finally {
      setSubmitting(false);
    }
  };

  if (!canCreate) {
    return (
      <div className="py-12 text-center">
        <p className="text-sm text-gray-500">You need experimenter permissions to create flags.</p>
      </div>
    );
  }

  return (
    <div>
      <div className="mb-2">
        <Link href="/flags" className="text-sm text-indigo-600 hover:text-indigo-800">
          &larr; Back to Flags
        </Link>
      </div>

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Create Feature Flag</h1>

      <form onSubmit={handleSubmit} className="max-w-lg rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
        <div className="mb-4">
          <label htmlFor="flag-name" className="mb-1 block text-sm font-medium text-gray-700">Name *</label>
          <input
            id="flag-name"
            type="text"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. dark_mode_rollout"
            className="w-full rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            data-testid="flag-name-input"
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
            data-testid="flag-desc-input"
          />
        </div>

        <div className="mb-4">
          <label htmlFor="flag-type" className="mb-1 block text-sm font-medium text-gray-700">Type</label>
          <select
            id="flag-type"
            value={type}
            onChange={(e) => setType(e.target.value as FlagType)}
            className="w-full rounded-md border border-gray-300 px-3 py-2 text-sm"
            data-testid="flag-type-select"
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
            data-testid="flag-default-input"
          />
        </div>

        <div className="mb-4 flex items-center gap-2">
          <input
            id="flag-enabled"
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
            data-testid="flag-enabled-input"
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
            data-testid="flag-rollout-input"
          />
        </div>

        {error && (
          <p className="mb-4 text-sm text-red-600" data-testid="create-error">{error}</p>
        )}

        <div className="flex gap-3">
          <button
            type="submit"
            disabled={submitting || !name.trim()}
            className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
            data-testid="create-submit"
          >
            {submitting ? 'Creating...' : 'Create Flag'}
          </button>
          <Link
            href="/flags"
            className="rounded-md border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
          >
            Cancel
          </Link>
        </div>
      </form>
    </div>
  );
}

export default function CreateFlagPage() {
  return <CreateFlagContent />;
}
