import { describe, test, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import FlagDetailPage from '@/app/flags/[id]/page';
import { ToastProvider } from '@/lib/toast-context';
import { getFlag } from '@/lib/api';
import type { Flag } from '@/lib/types';

vi.mock('@/lib/api', () => ({
  getFlag: vi.fn(),
  promoteToExperiment: vi.fn(),
}));

const mockFlag: Flag = {
  flagId: 'test-flag-id',
  name: 'Test Flag',
  description: 'Test description',
  type: 'BOOLEAN',
  defaultValue: 'false',
  enabled: true,
  rolloutPercentage: 0.5,
  variants: [
    { variantId: 'control', value: 'false', trafficFraction: 0.5 },
    { variantId: 'treatment', value: 'true', trafficFraction: 0.5 },
  ],
};

// Mock AuthContext
vi.mock('@/lib/auth-context', () => ({
  useAuth: () => ({
    user: { email: 'viewer@example.com', role: 'viewer' },
    canAtLeast: (role: string) => role === 'viewer',
  }),
}));

vi.mock('next/navigation', () => ({
  useParams: () => ({ id: 'test-flag-id' }),
  useRouter: () => ({ push: vi.fn() }),
}));

describe('Flag Detail Permissions', () => {
  test('renders disabled Edit button with tooltip for insufficient roles', async () => {
    vi.mocked(getFlag).mockResolvedValue(mockFlag);

    render(
      <ToastProvider>
        <FlagDetailPage />
      </ToastProvider>
    );

    await waitFor(() => {
      expect(screen.getByTestId('flag-name')).toBeInTheDocument();
    });

    const editBtn = screen.getByTestId('edit-flag-disabled');
    expect(editBtn).toBeInTheDocument();
    expect(editBtn).toHaveAttribute('aria-disabled', 'true');
    expect(editBtn).toHaveAttribute('title', 'Requires Experimenter role (you are Viewer)');
  });
});
