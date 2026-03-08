'use client';

import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { useCallback } from 'react';
import type { CreateExperimentRequest } from '@/lib/types';
import { createExperiment } from '@/lib/api';
import { ExperimentForm } from '@/components/experiment-form';

export default function NewExperimentPage() {
  const router = useRouter();

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

      <h1 className="mb-6 text-2xl font-bold text-gray-900">Create Experiment</h1>

      <div className="rounded-lg border border-gray-200 bg-white p-6">
        <ExperimentForm onSubmit={handleSubmit} />
      </div>
    </div>
  );
}
