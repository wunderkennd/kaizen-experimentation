import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { AuditLogTable } from '@/components/audit-log-table';
import { ToastProvider } from '@/lib/toast-context';
import type { AuditLogEntry } from '@/lib/types';

// Mock next/link
vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

const mockEntries: AuditLogEntry[] = [
  {
    entryId: 'audit-001',
    experimentId: 'exp-123',
    experimentName: 'Test Experiment',
    action: 'CONFIG_CHANGED',
    actorEmail: 'alice@example.com',
    timestamp: '2026-03-24T10:00:00Z',
    details: 'Changed traffic allocation',
    previousValue: '0.5',
    newValue: '0.6',
  },
  {
    entryId: 'audit-002',
    experimentId: 'exp-456',
    experimentName: 'Other Experiment',
    action: 'STARTED',
    actorEmail: 'bob@example.com',
    timestamp: '2026-03-24T11:00:00Z',
    details: 'Experiment started',
  },
];

describe('AuditLogTable Copy Buttons', () => {
  it('renders copy buttons when expanded', async () => {
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <AuditLogTable entries={mockEntries} />
      </ToastProvider>
    );

    // Initially not expanded
    expect(screen.queryByText('Experiment ID:')).not.toBeInTheDocument();

    // Expand the first row
    const row = screen.getByTestId('audit-row-audit-001');
    await user.click(row);

    // Check Experiment ID and its copy button
    expect(screen.getByText('Experiment ID:')).toBeInTheDocument();
    expect(screen.getByText('exp-123')).toBeInTheDocument();
    expect(screen.getByLabelText('Copy experiment ID')).toBeInTheDocument();

    // Check Previous value and its copy button
    expect(screen.getByText('Previous:')).toBeInTheDocument();
    expect(screen.getByText('0.5')).toBeInTheDocument();
    expect(screen.getByLabelText('Copy previous value')).toBeInTheDocument();

    // Check New value and its copy button
    expect(screen.getByText('New:')).toBeInTheDocument();
    expect(screen.getByText('0.6')).toBeInTheDocument();
    expect(screen.getByLabelText('Copy new value')).toBeInTheDocument();
  });

  it('renders experiment ID copy button even for entries without values', async () => {
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <AuditLogTable entries={mockEntries} />
      </ToastProvider>
    );

    // Expand the second row (STARTED)
    const row = screen.getByTestId('audit-row-audit-002');
    await user.click(row);

    // Check Experiment ID and its copy button
    expect(screen.getByText('Experiment ID:')).toBeInTheDocument();
    expect(screen.getByText('exp-456')).toBeInTheDocument();
    expect(screen.getByLabelText('Copy experiment ID')).toBeInTheDocument();

    // Should NOT show Previous/New labels
    expect(screen.queryByText('Previous:')).not.toBeInTheDocument();
    expect(screen.queryByText('New:')).not.toBeInTheDocument();
  });

  it('calls clipboard.writeText when copy buttons are clicked', async () => {
    const user = userEvent.setup();
    const writeTextSpy = vi.spyOn(navigator.clipboard, 'writeText');

    render(
      <ToastProvider>
        <AuditLogTable entries={mockEntries} />
      </ToastProvider>
    );

    const row = screen.getByTestId('audit-row-audit-001');
    await user.click(row);

    const copyExpIdBtn = screen.getByLabelText('Copy experiment ID');
    await user.click(copyExpIdBtn);
    expect(writeTextSpy).toHaveBeenCalledWith('exp-123');

    const copyPrevBtn = screen.getByLabelText('Copy previous value');
    await user.click(copyPrevBtn);
    expect(writeTextSpy).toHaveBeenCalledWith('0.5');

    const copyNewBtn = screen.getByLabelText('Copy new value');
    await user.click(copyNewBtn);
    expect(writeTextSpy).toHaveBeenCalledWith('0.6');

    writeTextSpy.mockRestore();
  });
});
