'use client';

import Link from 'next/link';

export function NavHeader() {
  return (
    <header className="border-b border-gray-200 bg-white">
      <div className="mx-auto flex h-14 max-w-7xl items-center px-4 sm:px-6 lg:px-8">
        <Link href="/" className="text-lg font-semibold text-gray-900">
          Experimentation Platform
        </Link>
      </div>
    </header>
  );
}
