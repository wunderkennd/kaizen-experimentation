'use client';

import { useParams } from 'next/navigation';
import Link from 'next/link';

export default function ResultsPage() {
  const params = useParams<{ id: string }>();

  return (
    <div>
      <nav className="mb-4 text-sm text-gray-500">
        <Link href="/" className="hover:text-indigo-600">Experiments</Link>
        <span className="mx-2">/</span>
        <Link href={`/experiments/${params.id}`} className="hover:text-indigo-600">Detail</Link>
        <span className="mx-2">/</span>
        <span className="text-gray-900">Results</span>
      </nav>
      <h1 className="text-2xl font-bold text-gray-900">Results Dashboard</h1>
      <p className="mt-2 text-sm text-gray-500">
        Results visualization will be implemented in Phase 2 when M4a analysis APIs are available.
      </p>
    </div>
  );
}
