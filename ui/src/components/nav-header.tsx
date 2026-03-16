'use client';

import Link from 'next/link';
import dynamic from 'next/dynamic';
import { useAuth } from '@/lib/auth-context';
import { ALL_ROLES, ROLE_LABELS, ROLE_BADGE_COLORS } from '@/lib/auth';
import type { UserRole } from '@/lib/auth';

const ConnectionStatus = dynamic(
  () => import('@/components/connection-status').then((m) => ({ default: m.ConnectionStatus })),
  { ssr: false },
);

export function NavHeader() {
  const { user, setDevRole, isDevMode } = useAuth();

  return (
    <header className="border-b border-gray-200 bg-white">
      <nav aria-label="Main navigation" className="mx-auto flex h-14 max-w-7xl items-center justify-between px-4 sm:px-6 lg:px-8">
        <div className="flex items-center gap-6">
          <Link href="/" className="text-lg font-semibold text-gray-900">
            Experimentation Platform
          </Link>
          <Link href="/metrics" className="text-sm font-medium text-gray-600 hover:text-gray-900" data-testid="nav-metrics">
            Metrics
          </Link>
        </div>

        <div className="flex items-center gap-3">
          <ConnectionStatus />
          {isDevMode && (
            <select
              value={user.role}
              onChange={(e) => setDevRole(e.target.value as UserRole)}
              className="rounded border border-dashed border-orange-400 bg-orange-50 px-2 py-1 text-xs text-orange-700"
              data-testid="dev-role-switcher"
              aria-label="Dev role switcher"
            >
              {ALL_ROLES.map((r) => (
                <option key={r} value={r}>{ROLE_LABELS[r]}</option>
              ))}
            </select>
          )}
          <span
            className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium ${ROLE_BADGE_COLORS[user.role]}`}
            data-testid="role-badge"
          >
            {ROLE_LABELS[user.role]}
          </span>
          <span className="text-sm text-gray-600" data-testid="user-email">
            {user.email}
          </span>
        </div>
      </nav>
    </header>
  );
}
