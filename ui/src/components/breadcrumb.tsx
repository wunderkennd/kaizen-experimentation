'use client';

import Link from 'next/link';

export interface BreadcrumbItem {
  label: string;
  href?: string;
}

interface BreadcrumbProps {
  items: BreadcrumbItem[];
}

export function Breadcrumb({ items }: BreadcrumbProps) {
  return (
    <nav className="mb-4 text-sm text-gray-500" aria-label="Breadcrumb">
      <ol className="flex items-center">
        {items.map((item, index) => (
          <li key={item.label} className="flex items-center">
            {index > 0 && <span className="mx-2" aria-hidden="true">/</span>}
            {item.href ? (
              <Link href={item.href} className="hover:text-indigo-600">
                {item.label}
              </Link>
            ) : (
              <span className="text-gray-900" aria-current={index === items.length - 1 ? 'page' : undefined}>
                {item.label}
              </span>
            )}
          </li>
        ))}
      </ol>
    </nav>
  );
}
