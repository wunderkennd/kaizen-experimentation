import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { hasAtLeast, isValidRole } from '@/lib/auth';
import { AuthProvider, useAuth } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';

// --- Pure utility tests ---

describe('hasAtLeast', () => {
  it('admin >= all roles', () => {
    expect(hasAtLeast('admin', 'viewer')).toBe(true);
    expect(hasAtLeast('admin', 'analyst')).toBe(true);
    expect(hasAtLeast('admin', 'experimenter')).toBe(true);
    expect(hasAtLeast('admin', 'admin')).toBe(true);
  });

  it('viewer < experimenter', () => {
    expect(hasAtLeast('viewer', 'experimenter')).toBe(false);
  });

  it('experimenter < admin', () => {
    expect(hasAtLeast('experimenter', 'admin')).toBe(false);
  });

  it('analyst >= viewer', () => {
    expect(hasAtLeast('analyst', 'viewer')).toBe(true);
  });
});

describe('isValidRole', () => {
  it('accepts valid roles', () => {
    expect(isValidRole('viewer')).toBe(true);
    expect(isValidRole('analyst')).toBe(true);
    expect(isValidRole('experimenter')).toBe(true);
    expect(isValidRole('admin')).toBe(true);
  });

  it('rejects invalid roles', () => {
    expect(isValidRole('superadmin')).toBe(false);
    expect(isValidRole('')).toBe(false);
  });
});

// --- AuthContext tests ---

function TestConsumer() {
  const { user, canAtLeast } = useAuth();
  return (
    <div>
      <span data-testid="email">{user.email}</span>
      <span data-testid="role">{user.role}</span>
      <span data-testid="can-create">{canAtLeast('experimenter') ? 'yes' : 'no'}</span>
      <span data-testid="can-admin">{canAtLeast('admin') ? 'yes' : 'no'}</span>
    </div>
  );
}

describe('AuthContext', () => {
  it('provides initialUser to consumers', () => {
    const user: AuthUser = { email: 'alice@test.com', role: 'analyst' };
    render(
      <AuthProvider initialUser={user}>
        <TestConsumer />
      </AuthProvider>,
    );

    expect(screen.getByTestId('email')).toHaveTextContent('alice@test.com');
    expect(screen.getByTestId('role')).toHaveTextContent('analyst');
  });

  it('canAtLeast works correctly', () => {
    const user: AuthUser = { email: 'exp@test.com', role: 'experimenter' };
    render(
      <AuthProvider initialUser={user}>
        <TestConsumer />
      </AuthProvider>,
    );

    expect(screen.getByTestId('can-create')).toHaveTextContent('yes');
    expect(screen.getByTestId('can-admin')).toHaveTextContent('no');
  });

  it('admin can do everything', () => {
    const user: AuthUser = { email: 'admin@test.com', role: 'admin' };
    render(
      <AuthProvider initialUser={user}>
        <TestConsumer />
      </AuthProvider>,
    );

    expect(screen.getByTestId('can-create')).toHaveTextContent('yes');
    expect(screen.getByTestId('can-admin')).toHaveTextContent('yes');
  });

  it('throws outside provider', () => {
    // Suppress React error boundary output
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => render(<TestConsumer />)).toThrow('useAuth must be used within an AuthProvider');
    spy.mockRestore();
  });
});

// --- Page-level RBAC tests ---

// Mock next/navigation for page tests
vi.mock('next/navigation', () => ({
  useParams: () => ({ id: '22222222-2222-2222-2222-222222222222' }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/',
}));

vi.mock('@/lib/toast-context', () => ({
  useToast: () => ({ addToast: vi.fn(), removeToast: vi.fn(), toasts: [] }),
  ToastProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

describe('List page RBAC', () => {
  // Lazy import to avoid hoisting issues with vi.mock
  let ExperimentListPage: () => React.JSX.Element;

  beforeAll(async () => {
    const mod = await import('@/app/page');
    ExperimentListPage = mod.default;
  });

  it('"New Experiment" is disabled for viewer', async () => {
    const viewer: AuthUser = { email: 'v@test.com', role: 'viewer' };
    render(
      <AuthProvider initialUser={viewer}>
        <ExperimentListPage />
      </AuthProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('new-experiment-disabled')).toBeInTheDocument();
    });
  });

  it('"New Experiment" is a link for experimenter', async () => {
    const exp: AuthUser = { email: 'e@test.com', role: 'experimenter' };
    render(
      <AuthProvider initialUser={exp}>
        <ExperimentListPage />
      </AuthProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTestId('new-experiment-link')).toBeInTheDocument();
    });
  });
});

describe('Detail page RBAC', () => {
  let ExperimentDetailPage: () => React.JSX.Element;

  beforeAll(async () => {
    const mod = await import('@/app/experiments/[id]/page');
    ExperimentDetailPage = mod.default;
  });

  it('shows VariantForm for experimenter on DRAFT', async () => {
    const exp: AuthUser = { email: 'e@test.com', role: 'experimenter' };
    render(
      <AuthProvider initialUser={exp}>
        <ExperimentDetailPage />
      </AuthProvider>,
    );

    await waitFor(() => {
      // DRAFT experiment (22222222) should show editable form
      expect(screen.getByText('Save Variants')).toBeInTheDocument();
    });
  });

  it('shows VariantTable (read-only) for viewer on DRAFT', async () => {
    const viewer: AuthUser = { email: 'v@test.com', role: 'viewer' };
    render(
      <AuthProvider initialUser={viewer}>
        <ExperimentDetailPage />
      </AuthProvider>,
    );

    await waitFor(() => {
      expect(screen.getByText('control')).toBeInTheDocument();
    });
    // Should NOT show Save Variants button
    expect(screen.queryByText('Save Variants')).not.toBeInTheDocument();
  });
});

describe('New experiment page RBAC', () => {
  let NewExperimentPage: () => React.JSX.Element;

  beforeAll(async () => {
    const mod = await import('@/app/experiments/new/page');
    NewExperimentPage = mod.default;
  });

  it('shows "Insufficient Permissions" for viewer', () => {
    const viewer: AuthUser = { email: 'v@test.com', role: 'viewer' };
    render(
      <AuthProvider initialUser={viewer}>
        <NewExperimentPage />
      </AuthProvider>,
    );

    expect(screen.getByTestId('insufficient-permissions')).toBeInTheDocument();
    expect(screen.getByText('Insufficient Permissions')).toBeInTheDocument();
  });

  it('shows experiment form for experimenter', () => {
    const exp: AuthUser = { email: 'e@test.com', role: 'experimenter' };
    render(
      <AuthProvider initialUser={exp}>
        <NewExperimentPage />
      </AuthProvider>,
    );

    expect(screen.getByRole('heading', { name: 'Create Experiment' })).toBeInTheDocument();
    expect(screen.queryByTestId('insufficient-permissions')).not.toBeInTheDocument();
  });
});

describe('NavHeader RBAC', () => {
  let NavHeader: () => React.JSX.Element;

  beforeAll(async () => {
    const mod = await import('@/components/nav-header');
    NavHeader = mod.NavHeader;
  });

  it('shows role badge', () => {
    const user: AuthUser = { email: 'test@x.com', role: 'experimenter' };
    render(
      <AuthProvider initialUser={user}>
        <NavHeader />
      </AuthProvider>,
    );

    expect(screen.getByTestId('role-badge')).toHaveTextContent('Experimenter');
  });

  it('shows user email', () => {
    const user: AuthUser = { email: 'alice@streamco.com', role: 'admin' };
    render(
      <AuthProvider initialUser={user}>
        <NavHeader />
      </AuthProvider>,
    );

    expect(screen.getByTestId('user-email')).toHaveTextContent('alice@streamco.com');
  });
});
