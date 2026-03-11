/**
 * RBAC utilities for the experimentation UI.
 * Mirrors the 4-level role hierarchy from Agent-5's Go interceptor
 * (services/management/internal/auth/identity.go).
 *
 * This is optimistic client-side gating only — the backend enforces security.
 */

export type UserRole = 'viewer' | 'analyst' | 'experimenter' | 'admin';

const ROLE_LEVELS: Record<UserRole, number> = {
  viewer: 0,
  analyst: 1,
  experimenter: 2,
  admin: 3,
};

/** Returns true if `role` is at least as privileged as `required`. */
export function hasAtLeast(role: UserRole, required: UserRole): boolean {
  return ROLE_LEVELS[role] >= ROLE_LEVELS[required];
}

/** Type guard: checks if a string is a valid UserRole. */
export function isValidRole(s: string): s is UserRole {
  return s in ROLE_LEVELS;
}

/** Maps UI actions to the minimum role required. */
export const UI_PERMISSIONS = {
  createExperiment: 'experimenter' as UserRole,
  updateExperiment: 'experimenter' as UserRole,
  startExperiment: 'experimenter' as UserRole,
  concludeExperiment: 'experimenter' as UserRole,
  pauseExperiment: 'experimenter' as UserRole,
  resumeExperiment: 'experimenter' as UserRole,
  archiveExperiment: 'admin' as UserRole,
} as const;

/** Human-readable labels for roles. */
export const ROLE_LABELS: Record<UserRole, string> = {
  viewer: 'Viewer',
  analyst: 'Analyst',
  experimenter: 'Experimenter',
  admin: 'Admin',
};

/** Tailwind badge color classes for each role. */
export const ROLE_BADGE_COLORS: Record<UserRole, string> = {
  viewer: 'bg-gray-100 text-gray-700',
  analyst: 'bg-blue-100 text-blue-700',
  experimenter: 'bg-green-100 text-green-700',
  admin: 'bg-purple-100 text-purple-700',
};

/** All roles in hierarchy order, useful for dropdowns. */
export const ALL_ROLES: UserRole[] = ['viewer', 'analyst', 'experimenter', 'admin'];
