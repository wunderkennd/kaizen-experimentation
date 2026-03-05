'use client';

import Link from 'next/link';
import type { Experiment } from '@/lib/types';
import { formatDate } from '@/lib/utils';
import { StateBadge } from './state-badge';
import { TypeBadge } from './type-badge';

interface ExperimentCardProps {
  experiment: Experiment;
}

export function ExperimentCard({ experiment }: ExperimentCardProps) {
  return (
    <tr className="hover:bg-gray-50">
      <td className="whitespace-nowrap px-4 py-3">
        <Link
          href={`/experiments/${experiment.experimentId}`}
          className="text-sm font-medium text-indigo-600 hover:text-indigo-800"
        >
          {experiment.name}
        </Link>
      </td>
      <td className="whitespace-nowrap px-4 py-3 text-sm text-gray-600">
        {experiment.ownerEmail}
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
        {experiment.state === 'CONCLUDED' && (
          <Link
            href={`/experiments/${experiment.experimentId}/results`}
            className="text-blue-600 hover:text-blue-800"
          >
            Results available
          </Link>
        )}
      </td>
    </tr>
  );
}
