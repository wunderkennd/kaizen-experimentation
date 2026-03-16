'use client';

import { useWizard } from '../wizard-context';

export function SessionConfigForm() {
  const { state, dispatch } = useWizard();
  const config = state.sessionConfig;

  const update = (partial: Partial<typeof config>) =>
    dispatch({ type: 'SET_FIELD', field: 'sessionConfig', value: { ...config, ...partial } });

  return (
    <div className="space-y-4">
      <div>
        <label htmlFor="session-id-attr" className="block text-sm font-medium text-gray-700">
          Session ID Attribute <span className="text-red-500">*</span>
        </label>
        <input
          id="session-id-attr"
          type="text"
          value={config.sessionIdAttribute}
          onChange={(e) => update({ sessionIdAttribute: e.target.value })}
          placeholder="e.g., session_id"
          className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        />
      </div>

      <div>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={config.allowCrossSessionVariation}
            onChange={(e) => update({ allowCrossSessionVariation: e.target.checked })}
            className="rounded border-gray-300"
          />
          <span className="text-sm text-gray-700">Allow cross-session variation</span>
        </label>
      </div>

      <div>
        <label htmlFor="min-sessions" className="block text-sm font-medium text-gray-700">
          Minimum Sessions Per User
        </label>
        <input
          id="min-sessions"
          type="number"
          min={1}
          value={config.minSessionsPerUser}
          onChange={(e) => update({ minSessionsPerUser: parseInt(e.target.value) || 1 })}
          className="mt-1 block w-32 rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        />
      </div>
    </div>
  );
}
