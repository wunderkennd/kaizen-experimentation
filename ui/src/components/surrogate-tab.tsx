'use client';

import type { SurrogateProjection } from '@/lib/types';

interface SurrogateTabProps {
  projections: SurrogateProjection[];
}

function rSquaredBadge(r2: number): { label: string; bg: string; text: string } {
  if (r2 >= 0.7) return { label: 'High', bg: 'bg-green-100', text: 'text-green-800' };
  if (r2 >= 0.5) return { label: 'Medium', bg: 'bg-yellow-100', text: 'text-yellow-800' };
  return { label: 'Low', bg: 'bg-red-100', text: 'text-red-800' };
}

export function SurrogateTab({ projections }: SurrogateTabProps) {
  if (projections.length === 0) {
    return (
      <div className="rounded-md bg-gray-50 p-4 text-sm text-gray-500">
        No surrogate projections available for this experiment.
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="rounded-md bg-blue-50 border border-blue-200 p-4">
        <h4 className="text-sm font-semibold text-blue-800">Surrogate Projections</h4>
        <p className="mt-1 text-sm text-blue-700">
          Long-term metric effects projected from short-term surrogate metrics.
          Confidence depends on model calibration (R&sup2;).
        </p>
      </div>

      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-gray-200">
          <thead>
            <tr className="bg-gray-50">
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Long-Term Metric</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Surrogate Metric</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">Projected Effect</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">95% CI</th>
              <th className="px-4 py-3 text-right text-xs font-medium uppercase text-gray-500">R&sup2;</th>
              <th className="px-4 py-3 text-center text-xs font-medium uppercase text-gray-500">Confidence</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {projections.map((proj) => {
              const badge = rSquaredBadge(proj.calibrationRSquared);
              const ciIncludes0 = proj.projectionCiLower <= 0 && proj.projectionCiUpper >= 0;
              return (
                <tr key={`${proj.metricId}-${proj.surrogateMetricId}`}>
                  <td className="whitespace-nowrap px-4 py-3 text-sm font-medium text-gray-900">
                    {proj.metricId}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
                    {proj.surrogateMetricId}
                  </td>
                  <td className={`whitespace-nowrap px-4 py-3 text-right text-sm font-mono ${
                    proj.projectedEffect > 0 ? 'text-green-700' : proj.projectedEffect < 0 ? 'text-red-700' : 'text-gray-700'
                  }`}>
                    {proj.projectedEffect > 0 ? '+' : ''}{proj.projectedEffect.toFixed(4)}
                  </td>
                  <td className={`whitespace-nowrap px-4 py-3 text-right text-sm font-mono ${
                    ciIncludes0 ? 'text-gray-500' : 'text-gray-700'
                  }`}>
                    [{proj.projectionCiLower.toFixed(4)}, {proj.projectionCiUpper.toFixed(4)}]
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-right text-sm font-mono text-gray-700">
                    {proj.calibrationRSquared.toFixed(2)}
                  </td>
                  <td className="whitespace-nowrap px-4 py-3 text-center">
                    <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${badge.bg} ${badge.text}`}>
                      {badge.label}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-4">
        <h4 className="text-sm font-semibold text-gray-900">Calibration Guide</h4>
        <div className="mt-2 flex items-center gap-6 text-xs text-gray-600">
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-green-500" aria-hidden="true" />
            High (R&sup2; &ge; 0.7)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-yellow-500" aria-hidden="true" />
            Medium (0.5 &le; R&sup2; &lt; 0.7)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-2.5 w-2.5 rounded-full bg-red-500" aria-hidden="true" />
            Low (R&sup2; &lt; 0.5)
          </span>
        </div>
      </div>
    </div>
  );
}
