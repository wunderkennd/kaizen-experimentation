'use client';

interface ProgressStep {
  label: string;
  status: 'done' | 'in_progress' | 'pending';
}

const PROGRESS_STEPS: ProgressStep[] = [
  { label: 'Stopping traffic', status: 'done' },
  { label: 'Running final analysis', status: 'in_progress' },
  { label: 'Generating report', status: 'pending' },
];

export function ConcludingProgress() {
  return (
    <div className="rounded-lg border border-orange-200 bg-orange-50 p-4">
      <h3 className="text-sm font-semibold text-orange-800">Concluding Experiment</h3>
      <p className="mt-1 text-xs text-orange-700">
        Finalizing analysis and generating results...
      </p>
      <div className="mt-4 flex items-center gap-2">
        {PROGRESS_STEPS.map((step, i) => (
          <div key={step.label} className="flex items-center gap-2">
            {i > 0 && (
              <div
                className={`h-0.5 w-8 ${step.status === 'pending' ? 'bg-gray-300' : 'bg-orange-400'}`}
              />
            )}
            <div className="flex flex-col items-center">
              <div
                className={`flex h-6 w-6 items-center justify-center rounded-full text-xs font-medium ${
                  step.status === 'done'
                    ? 'bg-green-100 text-green-700'
                    : step.status === 'in_progress'
                      ? 'animate-pulse bg-orange-200 text-orange-800'
                      : 'bg-gray-200 text-gray-500'
                }`}
                data-testid={`step-${step.status}`}
              >
                {step.status === 'done' ? '✓' : i + 1}
              </div>
              <span className="mt-1 text-xs text-gray-700 whitespace-nowrap">{step.label}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
