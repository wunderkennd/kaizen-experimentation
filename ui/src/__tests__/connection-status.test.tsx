import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ConnectionStatus } from '@/components/connection-status';
import type { HealthStatus } from '@/lib/health';

// Mock the health hook
const mockUseHealthCheck = vi.fn();
vi.mock('@/lib/health', () => ({
  useHealthCheck: (...args: unknown[]) => mockUseHealthCheck(...args),
}));

describe('ConnectionStatus', () => {
  beforeEach(() => {
    mockUseHealthCheck.mockReset();
  });

  it('shows "Mock" badge when in mock mode', () => {
    mockUseHealthCheck.mockReturnValue({
      status: null,
      checking: false,
      isMockMode: true,
    });

    render(<ConnectionStatus />);
    const badge = screen.getByTestId('connection-status');
    expect(badge).toHaveTextContent('Mock');
    expect(badge.className).toContain('yellow');
  });

  it('shows "Connected" with green styling when all services healthy', () => {
    const healthyStatus: HealthStatus = {
      services: [{ name: 'Management', url: '/api', healthy: true, latencyMs: 12 }],
      allHealthy: true,
      checkedAt: '2026-03-12T00:00:00Z',
    };
    mockUseHealthCheck.mockReturnValue({
      status: healthyStatus,
      checking: false,
      isMockMode: false,
    });

    render(<ConnectionStatus />);
    const badge = screen.getByTestId('connection-status');
    expect(badge).toHaveTextContent('Connected');
    expect(badge.className).toContain('green');
  });

  it('shows "Disconnected" with red styling on failure', () => {
    const unhealthyStatus: HealthStatus = {
      services: [{ name: 'Management', url: '/api', healthy: false, latencyMs: null, error: 'Timeout' }],
      allHealthy: false,
      checkedAt: '2026-03-12T00:00:00Z',
    };
    mockUseHealthCheck.mockReturnValue({
      status: unhealthyStatus,
      checking: false,
      isMockMode: false,
    });

    render(<ConnectionStatus />);
    const badge = screen.getByTestId('connection-status');
    expect(badge).toHaveTextContent('Disconnected');
    expect(badge.className).toContain('red');
    expect(badge.getAttribute('title')).toContain('Management: Timeout');
  });

  it('shows "Checking..." during initial check', () => {
    mockUseHealthCheck.mockReturnValue({
      status: null,
      checking: true,
      isMockMode: false,
    });

    render(<ConnectionStatus />);
    const badge = screen.getByTestId('connection-status');
    expect(badge).toHaveTextContent('Checking...');
    expect(badge.className).toContain('gray');
  });

  it('has aria-live="polite" for screen reader announcements', () => {
    mockUseHealthCheck.mockReturnValue({
      status: null,
      checking: true,
      isMockMode: false,
    });

    render(<ConnectionStatus />);
    const badge = screen.getByTestId('connection-status');
    expect(badge.getAttribute('aria-live')).toBe('polite');
  });
});
