'use client';

import { useWizard } from '../wizard-context';

const QOE_METRICS = [
  { id: 'rebuffer_ratio', label: 'Rebuffer Ratio' },
  { id: 'time_to_first_frame_ms', label: 'Time to First Frame (ms)' },
  { id: 'avg_bitrate_kbps', label: 'Avg Bitrate (kbps)' },
  { id: 'resolution_switches', label: 'Resolution Switches' },
  { id: 'startup_failure_rate', label: 'Startup Failure Rate' },
];

export function QoeConfigForm() {
  const { state, dispatch } = useWizard();
  const config = state.qoeConfig;

  const update = (partial: Partial<typeof config>) =>
    dispatch({ type: 'SET_FIELD', field: 'qoeConfig', value: { ...config, ...partial } });

  const toggleMetric = (metricId: string) => {
    const metrics = config.qoeMetrics.includes(metricId)
      ? config.qoeMetrics.filter((m) => m !== metricId)
      : [...config.qoeMetrics, metricId];
    update({ qoeMetrics: metrics });
  };

  return (
    <div className="space-y-4">
      <div>
        <label className="block text-sm font-medium text-gray-700">
          QoE Metrics <span className="text-red-500">*</span>
        </label>
        <p className="mb-2 text-xs text-gray-500">Select the QoE metrics to monitor in this experiment.</p>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {QOE_METRICS.map((metric) => (
            <label key={metric.id} className="flex items-center gap-2 rounded border border-gray-200 p-2">
              <input
                type="checkbox"
                checked={config.qoeMetrics.includes(metric.id)}
                onChange={() => toggleMetric(metric.id)}
                className="rounded border-gray-300"
              />
              <span className="text-sm text-gray-700">{metric.label}</span>
            </label>
          ))}
        </div>
      </div>

      <div>
        <label htmlFor="device-filter" className="block text-sm font-medium text-gray-700">
          Device Filter
        </label>
        <input
          id="device-filter"
          type="text"
          value={config.deviceFilter}
          onChange={(e) => update({ deviceFilter: e.target.value })}
          placeholder="e.g., smart_tv, mobile (optional)"
          className="mt-1 block w-full rounded-md border border-gray-300 px-3 py-2 text-sm shadow-sm focus:border-indigo-500 focus:ring-indigo-500"
        />
      </div>
    </div>
  );
}
