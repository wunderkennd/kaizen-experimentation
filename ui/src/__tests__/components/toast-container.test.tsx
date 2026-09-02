import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ToastContainer } from '@/components/toast-container';
import { ToastProvider, useToast } from '@/lib/toast-context';
import { useEffect } from 'react';

function ToastTestTrigger({ message, variant }: { message: string; variant?: 'success' | 'error' | 'info' | 'warning' }) {
  const { addToast } = useToast();
  useEffect(() => {
    addToast(message, variant);
  }, [addToast, message, variant]);
  return null;
}

describe('ToastContainer', () => {
  it('renders nothing when there are no toasts', () => {
    const { container } = render(
      <ToastProvider>
        <ToastContainer />
      </ToastProvider>
    );
    expect(container.firstChild).toBeNull();
  });

  it('renders toast notification with accessible dismiss button and focus styles', async () => {
    render(
      <ToastProvider>
        <ToastTestTrigger message="Operation successful!" variant="success" />
        <ToastContainer />
      </ToastProvider>
    );

    const toastMessage = await screen.findByText('Operation successful!');
    expect(toastMessage).toBeInTheDocument();

    const dismissButton = screen.getByRole('button', { name: 'Dismiss notification' });
    expect(dismissButton).toBeInTheDocument();
    expect(dismissButton).toHaveClass('focus-visible:ring-2');
    expect(dismissButton).toHaveClass('focus-visible:ring-indigo-500');
  });

  it('dismisses toast when dismiss button is clicked', async () => {
    const user = userEvent.setup();
    render(
      <ToastProvider>
        <ToastTestTrigger message="Dismissable notification" />
        <ToastContainer />
      </ToastProvider>
    );

    const dismissButton = await screen.findByRole('button', { name: 'Dismiss notification' });
    await user.click(dismissButton);

    expect(screen.queryByText('Dismissable notification')).not.toBeInTheDocument();
  });
});
