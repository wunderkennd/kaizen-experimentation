'use client';

import { WIZARD_STEPS } from './wizard-context';

interface StepIndicatorProps {
  currentStep: number;
  onStepClick?: (step: number) => void;
}

export function StepIndicator({ currentStep, onStepClick }: StepIndicatorProps) {
  return (
    <nav aria-label="Wizard progress" className="mb-8">
      <ol className="flex items-center">
        {WIZARD_STEPS.map((label, i) => {
          const isCompleted = i < currentStep;
          const isCurrent = i === currentStep;

          return (
            <li key={label} className="flex items-center">
              {i > 0 && (
                <div
                  className={`mx-2 h-0.5 w-8 sm:w-12 ${isCompleted ? 'bg-indigo-600' : 'bg-gray-300'}`}
                  aria-hidden="true"
                />
              )}
              <button
                type="button"
                onClick={() => onStepClick?.(i)}
                disabled={!onStepClick}
                aria-current={isCurrent ? 'step' : undefined}
                className={`flex items-center gap-2 rounded-full px-3 py-1.5 text-xs font-medium transition-colors ${
                  isCurrent
                    ? 'bg-indigo-600 text-white'
                    : isCompleted
                      ? 'bg-indigo-100 text-indigo-700 hover:bg-indigo-200'
                      : 'bg-gray-100 text-gray-500'
                } ${onStepClick ? 'cursor-pointer' : 'cursor-default'}`}
              >
                <span
                  className={`flex h-5 w-5 items-center justify-center rounded-full text-xs ${
                    isCurrent
                      ? 'bg-white text-indigo-600'
                      : isCompleted
                        ? 'bg-indigo-600 text-white'
                        : 'bg-gray-300 text-gray-600'
                  }`}
                >
                  {isCompleted ? '\u2713' : i + 1}
                </span>
                <span className="hidden sm:inline">{label}</span>
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
