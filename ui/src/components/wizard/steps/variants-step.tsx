'use client';

import { useWizard } from '../wizard-context';
import { validateJsonPayload } from '@/lib/validation';
import { formatPercent } from '@/lib/utils';

export function VariantsStep() {
  const { state, dispatch } = useWizard();
  const { variants } = state;

  const trafficSum = variants.reduce((acc, v) => acc + v.trafficFraction, 0);
  const trafficSumValid = Math.abs(trafficSum - 1.0) < 1e-9;

  return (
    <section>
      <h2 className="mb-4 text-lg font-semibold text-gray-900">Variants</h2>
      <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Name</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Traffic</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Control</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Payload JSON</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500" />
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {variants.map((v, i) => (
              <tr key={v.variantId}>
                <td className="px-4 py-2">
                  <input
                    type="text"
                    value={v.name}
                    onChange={(e) => dispatch({ type: 'UPDATE_VARIANT', index: i, field: 'name', value: e.target.value })}
                    aria-label={`Variant ${i + 1} name`}
                    className="w-full rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </td>
                <td className="px-4 py-2">
                  <input
                    type="number"
                    min={0}
                    max={1}
                    step={0.01}
                    value={v.trafficFraction}
                    onChange={(e) => dispatch({ type: 'UPDATE_VARIANT', index: i, field: 'trafficFraction', value: parseFloat(e.target.value) || 0 })}
                    aria-label={`Variant ${i + 1} traffic`}
                    className="w-24 rounded border border-gray-300 px-2 py-1 text-sm"
                  />
                </td>
                <td className="px-4 py-2 text-center">
                  <input
                    type="radio"
                    name="wizard-control-variant"
                    checked={v.isControl}
                    onChange={() => dispatch({ type: 'UPDATE_VARIANT', index: i, field: 'isControl', value: true })}
                    aria-label={`Set ${v.name || `variant ${i + 1}`} as control`}
                  />
                </td>
                <td className="px-4 py-2">
                  <textarea
                    value={v.payloadJson}
                    onChange={(e) => dispatch({ type: 'UPDATE_VARIANT', index: i, field: 'payloadJson', value: e.target.value })}
                    aria-label={`Variant ${i + 1} payload`}
                    rows={1}
                    className={`w-full rounded border px-2 py-1 font-mono text-xs ${
                      !validateJsonPayload(v.payloadJson) ? 'border-red-500' : 'border-gray-300'
                    }`}
                  />
                </td>
                <td className="px-4 py-2">
                  <button
                    type="button"
                    onClick={() => dispatch({ type: 'REMOVE_VARIANT', index: i })}
                    disabled={variants.length <= 2}
                    className="text-sm text-red-600 hover:text-red-800 disabled:cursor-not-allowed disabled:text-gray-400"
                  >
                    Remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="mt-2 flex items-center justify-between">
        <span className={`text-sm font-medium ${trafficSumValid ? 'text-green-700' : 'text-red-700'}`}>
          Total traffic: {formatPercent(trafficSum)}
        </span>
        <button
          type="button"
          onClick={() => dispatch({ type: 'ADD_VARIANT' })}
          className="rounded-md border border-gray-300 bg-white px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
        >
          Add Variant
        </button>
      </div>
    </section>
  );
}
