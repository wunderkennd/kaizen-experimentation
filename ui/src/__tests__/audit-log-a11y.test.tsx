import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import AuditLogPage from '@/app/audit/page';
import { ToastProvider } from '@/lib/toast-context';

// Mock next/navigation
vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

// Mock next/link to render an anchor tag
vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

async function renderAndWait() {
  render(
    <ToastProvider>
      <AuditLogPage />
    </ToastProvider>
  );
  await waitFor(() => {
    expect(screen.getByRole('heading', { name: 'Audit Log' })).toBeInTheDocument();
  });
}

describe('Audit Log Accessibility', () => {
  it('toggles row expansion with Enter key', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const row = screen.getByTestId('audit-row-audit-001');
    expect(row).toHaveAttribute('role', 'button');
    expect(row).toHaveAttribute('tabIndex', '0');
    expect(row).toHaveAttribute('aria-expanded', 'false');

    // Focus row and press Enter
    await row.focus();
    await user.keyboard('{Enter}');

    expect(row).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByTestId('audit-detail-audit-001')).toBeInTheDocument();

    // Press Enter again to collapse
    await user.keyboard('{Enter}');
    expect(row).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('audit-detail-audit-001')).not.toBeInTheDocument();
  });

  it('toggles row expansion with Space key', async () => {
    const user = userEvent.setup();
    await renderAndWait();

    const row = screen.getByTestId('audit-row-audit-002');

    // Focus row and press Space
    await row.focus();
    await user.keyboard(' ');

    expect(row).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByTestId('audit-detail-audit-002')).toBeInTheDocument();

    // Press Space again to collapse
    await user.keyboard(' ');
    expect(row).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('audit-detail-audit-002')).not.toBeInTheDocument();
  });
});
