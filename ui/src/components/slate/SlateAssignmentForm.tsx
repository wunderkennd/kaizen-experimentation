'use client';

import { memo, useState, useCallback } from 'react';
import { getSlateAssignment } from '@/lib/api';
import type { SlateAssignmentResponse } from '@/lib/types';
import { SlateResultsPanel } from './SlateResultsPanel';

interface SlateAssignmentFormProps {
  experimentId: string;
}

const DEFAULT_CANDIDATES = [
  'item-action-001',
  'item-comedy-002',
  'item-drama-003',
  'item-thriller-004',
  'item-scifi-005',
  'item-docs-006',
  'item-animation-007',
  'item-romance-008',
  'item-horror-009',
  'item-family-010',
  'item-crime-011',
  'item-mystery-012',
].join(', ');

export const SlateAssignmentForm = memo(function SlateAssignmentForm({
  experimentId,
}: SlateAssignmentFormProps) {
  const [candidateText, setCandidateText] = useState(DEFAULT_CANDIDATES);
  const [nSlots, setNSlots] = useState(5);
  const [userId, setUserId] = useState('test-user-001');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<SlateAssignmentResponse | null>(null);

  const handleSubmit = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    const candidateItemIds = candidateText
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    if (candidateItemIds.length === 0) {
      setError('Enter at least one candidate item ID.');
      return;
    }
    if (nSlots > candidateItemIds.length) {
      setError(`n_slots (${nSlots}) cannot exceed number of candidates (${candidateItemIds.length}).`);
      return;
    }

    setLoading(true);
    setError(null);
    setResult(null);

    try {
      const response = await getSlateAssignment(experimentId, userId, candidateItemIds);
      setResult(response);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'GetSlateAssignment failed');
    } finally {
      setLoading(false);
    }
  }, [experimentId, userId, candidateText, nSlots]);

  return (
    <div data-testid="slate-assignment-form">
      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <label htmlFor="slate-user-id" className="block text-xs font-medium text-gray-700">
            User ID
          </label>
          <input
            id="slate-user-id"
            type="text"
            value={userId}
            onChange={(e) => setUserId(e.target.value)}
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-900 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            placeholder="user-id"
            required
          />
        </div>

        <div>
          <label htmlFor="slate-n-slots" className="block text-xs font-medium text-gray-700">
            Number of Slots (n_slots)
          </label>
          <input
            id="slate-n-slots"
            type="number"
            min={1}
            max={20}
            value={nSlots}
            onChange={(e) => setNSlots(Math.max(1, Math.min(20, Number(e.target.value))))}
            className="mt-1 block w-32 rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-900 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
          />
        </div>

        <div>
          <label htmlFor="slate-candidates" className="block text-xs font-medium text-gray-700">
            Candidate Item IDs{' '}
            <span className="font-normal text-gray-500">(comma-separated)</span>
          </label>
          <textarea
            id="slate-candidates"
            value={candidateText}
            onChange={(e) => setCandidateText(e.target.value)}
            rows={3}
            className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-1.5 font-mono text-sm text-gray-900 focus:border-indigo-500 focus:outline-none focus:ring-1 focus:ring-indigo-500"
            placeholder="item-001, item-002, item-003"
          />
        </div>

        {error && (
          <p className="text-sm text-red-600" role="alert">
            {error}
          </p>
        )}

        <button
          type="submit"
          disabled={loading}
          className="inline-flex items-center rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
        >
          {loading ? 'Requesting…' : 'Get Slate Assignment'}
        </button>
      </form>

      {result && (
        <div className="mt-6">
          <SlateResultsPanel response={result} />
        </div>
      )}
    </div>
  );
});
