'use client';

import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { useCallback } from 'react';
import type { CreateExperimentRequest } from '@/lib/types';
import { createExperiment } from '@/lib/api';
import { ExperimentForm } from '@/components/experiment-form';
import { useAuth } from '@/lib/auth-context';
import { ROLE_LABELS } from '@/lib/auth';

export default function NewExperimentPage() {
  const router = useRouter();
  const { canAtLeast, user } = useAuth();
  const canCreate = canAtLeast('experimenter');

  const handleSubmit = useCallback(async (req: CreateExperimentRequest) => {
    const experiment = await createExperiment(req);
    router.push(`/experiments/${experiment.experimentId}`);
  }, [router]);

  return (
    <div>
      <nav className="mb-4 text-sm text-gray-500">
        <Link href="/" className="hover:text-indigo-600">Experiments</Link>
        <span className="mx-2">/</span>
        <span className="text-gray-900">New Experiment</span>
      </nav>

      {canCreate ? (
        <>
          <h1 className="mb-6 text-2xl font-bold text-gray-900">Create Experiment</h1>
          <div className="rounded-lg border border-gray-200 bg-white p-6">
            <ExperimentForm onSubmit={handleSubmit} />
          </div>
        </>
      ) : (
        <div className="rounded-lg border border-yellow-300 bg-yellow-50 p-6" data-testid="insufficient-permissions">
          <h2 className="text-lg font-semibold text-yellow-800">Insufficient Permissions</h2>
          <p className="mt-2 text-sm text-yellow-700">
            Creating experiments requires the <strong>Experimenter</strong> role.
            You are currently a <strong>{ROLE_LABELS[user.role]}</strong>.
          </p>
          <Link
            href="/"
            className="mt-4 inline-block rounded-md bg-yellow-600 px-3 py-2 text-sm font-medium text-white hover:bg-yellow-700"
          >
            Back to Experiments
          </Link>
        </div>
      )}
    </div>
  );
}
