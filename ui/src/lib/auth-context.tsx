'use client';

import { createContext, useContext, useState, useEffect, useMemo, useCallback } from 'react';
import type { ReactNode } from 'react';
import type { UserRole } from './auth';
import { hasAtLeast, isValidRole } from './auth';
import { setApiAuth } from './api';

export interface AuthUser {
  email: string;
  role: UserRole;
}

interface AuthContextValue {
  user: AuthUser;
  /** Check if the current user's role is at least `required`. */
  canAtLeast: (required: UserRole) => boolean;
  /** Switch role in dev mode. No-op in production. */
  setDevRole: (role: UserRole) => void;
  /** True when running in development mode (role switcher available). */
  isDevMode: boolean;
}

const AuthContext = createContext<AuthContextValue | null>(null);

function getDefaultUser(): AuthUser {
  const email = process.env.NEXT_PUBLIC_USER_EMAIL || 'dev@streamco.com';
  const roleStr = process.env.NEXT_PUBLIC_USER_ROLE || 'experimenter';
  const role: UserRole = isValidRole(roleStr) ? roleStr : 'experimenter';
  return { email, role };
}

interface AuthProviderProps {
  children: ReactNode;
  /** Override user for testing — bypasses env vars. */
  initialUser?: AuthUser;
}

export function AuthProvider({ children, initialUser }: AuthProviderProps) {
  const [user, setUser] = useState<AuthUser>(initialUser ?? getDefaultUser);
  const isDevMode = process.env.NODE_ENV !== 'production';

  // Keep api.ts auth headers in sync
  useEffect(() => {
    setApiAuth(user.email, user.role);
  }, [user.email, user.role]);

  const canAtLeast = useCallback(
    (required: UserRole) => hasAtLeast(user.role, required),
    [user.role],
  );

  const setDevRole = useCallback(
    (role: UserRole) => {
      if (isDevMode) {
        setUser((prev) => ({ ...prev, role }));
      }
    },
    [isDevMode],
  );

  const value = useMemo<AuthContextValue>(
    () => ({ user, canAtLeast, setDevRole, isDevMode }),
    [user, canAtLeast, setDevRole, isDevMode],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return ctx;
}
