'use client';

import Link from 'next/link';
import type { Experiment } from '@/lib/types';
import { formatDate } from '@/lib/utils';
import { StateBadge } from './state-badge';
import { TypeBadge } from './type-badge';
import { CopyButton } from './copy-button';

interface ExperimentCardProps {
  experiment: Experiment;
}

function ResultsCell({ experiment }: { experiment: Experiment }) {
  switch (experiment.state) {
    case 'RUNNING':
      return (
        <Link
          href={`/experiments/${experiment.experimentId}/results`}
          className="text-orange-600 hover:text-orange-800"
        >
          Interim results
        </Link>
      );
    case 'CONCLUDED':
      return (
        <Link
          href={`/experiments/${experiment.experimentId}/results`}
          className="text-blue-600 hover:text-blue-800"
        >
          Results available
        </Link>
      );
    case 'CONCLUDING':
      return <span className="text-gray-400">Finalizing...</span>;
    default:
      return null;
  }
}

/** @deprecated Use ExperimentRow instead */
export const ExperimentCard = ExperimentRow;

export function ExperimentRow({ experiment }: ExperimentCardProps) {
  return (
    <tr className="group hover:bg-gray-50 focus-within:bg-gray-50 focus-within:outline-none focus-within:ring-2 focus-within:ring-inset focus-within:ring-indigo-500">
      <td className="whitespace-nowrap px-4 py-3">
        <div className="flex items-center gap-2">
          <Link
            href={`/experiments/${experiment.experimentId}`}
            className="text-sm font-medium text-indigo-600 hover:text-indigo-800"
          >
            {experiment.name}
          </Link>
          <code className="hidden text-[10px] text-gray-400 sm:inline">
            {experiment.experimentId.slice(0, 8)}
          </code>
          <CopyButton
            value={experiment.experimentId}
            label="Copy experiment ID"
            successMessage="Experiment ID copied"
            className="h-4 w-4 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100"
          />
        </div>
      </td>
      <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
        <div className="flex items-center gap-2">
          <span>{experiment.ownerEmail}</span>
          <CopyButton
            value={experiment.ownerEmail}
            label="Copy owner email"
            successMessage="Owner email copied"
            className="h-4 w-4 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100"
          />
        </div>
      </td>
      <td className="whitespace-nowrap px-4 py-3">
        <TypeBadge type={experiment.type} />
      </td>
      <td className="whitespace-nowrap px-4 py-3">
        <StateBadge state={experiment.state} />
      </td>
      <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-500">
        {formatDate(experiment.createdAt)}
      </td>
      <td className="whitespace-nowrap px-4 py-3 text-sm">
        <ResultsCell experiment={experiment} />
      </td>
    </tr>
  );
}
