'use client';

import { memo } from 'react';
import type { AnalysisResult, Experiment } from '@/lib/types';
import { formatDate } from '@/lib/utils';

interface ResultsSummaryProps {
  analysisResult: AnalysisResult;
  experiment: Experiment;
}

function ResultsSummaryInner({ analysisResult, experiment }: ResultsSummaryProps) {
  const significantCount = analysisResult.metricResults.filter((m) => m.isSignificant).length;
  const totalCount = analysisResult.metricResults.length;

  return (
    <div className="mb-6 flex items-center gap-6 rounded-lg border border-gray-200 bg-white px-4 py-3">
      <div>
        <span className="text-xs font-medium uppercase text-gray-500">Experiment</span>
        <p className="text-sm font-medium text-gray-900">{experiment.name}</p>
      </div>
      <div>
        <span className="text-xs font-medium uppercase text-gray-500">Computed</span>
        <p className="text-sm text-gray-900">{formatDate(analysisResult.computedAt)}</p>
      </div>
      <div>
        <span className="text-xs font-medium uppercase text-gray-500">Significant Metrics</span>
        <p className="text-sm text-gray-900">
          {significantCount} / {totalCount}
        </p>
      </div>
    </div>
  );
}

export const ResultsSummary = memo(ResultsSummaryInner);
