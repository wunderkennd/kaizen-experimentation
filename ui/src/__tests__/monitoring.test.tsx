import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { server } from '@/__mocks__/server';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import MonitoringPage from '@/app/monitoring/page';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';
const ANALYSIS_SVC = '*/experimentation.analysis.v1.AnalysisService';

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
  render(<MonitoringPage />);
  await waitFor(() => {
    expect(screen.getByText('Monitoring')).toBeInTheDocument();
    expect(screen.getByTestId('summary-cards')).toBeInTheDocument();
  });
}

describe('Monitoring Page', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders page heading', async () => {
    await renderAndWait();

    expect(screen.getByRole('heading', { name: 'Monitoring', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Active Experiments Summary' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Running Experiments Health' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Recent Guardrail Breaches' })).toBeInTheDocument();
  });

  it('shows summary cards with experiment counts by state', async () => {
    await renderAndWait();

    // Seed data: 3 RUNNING, 2 DRAFT, 1 STARTING, 1 CONCLUDING, 2 CONCLUDED, 1 ARCHIVED
    const runningCard = screen.getByTestId('summary-card-RUNNING');
    expect(within(runningCard).getByTestId('count-RUNNING')).toHaveTextContent('3');

    const draftCard = screen.getByTestId('summary-card-DRAFT');
    expect(within(draftCard).getByTestId('count-DRAFT')).toHaveTextContent('2');

    const startingCard = screen.getByTestId('summary-card-STARTING');
    expect(within(startingCard).getByTestId('count-STARTING')).toHaveTextContent('1');

    const concludingCard = screen.getByTestId('summary-card-CONCLUDING');
    expect(within(concludingCard).getByTestId('count-CONCLUDING')).toHaveTextContent('1');

    const concludedCard = screen.getByTestId('summary-card-CONCLUDED');
    expect(within(concludedCard).getByTestId('count-CONCLUDED')).toHaveTextContent('2');

    const archivedCard = screen.getByTestId('summary-card-ARCHIVED');
    expect(within(archivedCard).getByTestId('count-ARCHIVED')).toHaveTextContent('1');
  });

  it('shows running experiments in health table', async () => {
    await renderAndWait();

    const table = screen.getByTestId('health-table');
    expect(table).toBeInTheDocument();

    // 3 RUNNING experiments: homepage_recs_v2, search_ranking_interleave, recommendation_holdout_q1
    expect(within(table).getByText('homepage_recs_v2')).toBeInTheDocument();
    expect(within(table).getByText('search_ranking_interleave')).toBeInTheDocument();
    expect(within(table).getByText('recommendation_holdout_q1')).toBeInTheDocument();

    // Verify type labels
    expect(within(table).getByText('A/B Test')).toBeInTheDocument();
    expect(within(table).getByText('Interleaving')).toBeInTheDocument();
    expect(within(table).getByText('Cumulative Holdout')).toBeInTheDocument();

    // Verify owners (alice owns 2 running experiments: homepage_recs_v2 and recommendation_holdout_q1)
    expect(within(table).getAllByText('alice@streamco.com').length).toBe(2);
    expect(within(table).getByText('carol@streamco.com')).toBeInTheDocument();
  });

  it('shows guardrail breach information', async () => {
    await renderAndWait();

    const breachList = screen.getByTestId('breach-list');
    expect(breachList).toBeInTheDocument();

    // homepage_recs_v2 has 2 breaches
    const breachItems = within(breachList).getAllByTestId('breach-item');
    expect(breachItems.length).toBe(2);

    // Check breach details
    expect(within(breachList).getAllByText('homepage_recs_v2').length).toBe(2);
    expect(within(breachList).getAllByText(/crash_rate/).length).toBe(2);

    // Check action badges — one ALERT, one AUTO_PAUSE
    const actions = within(breachList).getAllByTestId('breach-action');
    const actionTexts = actions.map((a) => a.textContent);
    expect(actionTexts).toContain('Alert Only');
    expect(actionTexts).toContain('Auto-Paused');
  });

  it('auto-refresh toggle works', async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    await renderAndWait();

    const toggle = screen.getByTestId('auto-refresh-toggle');
    expect(toggle).not.toBeChecked();

    // Enable auto-refresh
    await user.click(toggle);
    expect(toggle).toBeChecked();

    // Disable auto-refresh
    await user.click(toggle);
    expect(toggle).not.toBeChecked();
  });

  it('links to experiment detail pages', async () => {
    await renderAndWait();

    const link = screen.getByTestId('experiment-link-11111111-1111-1111-1111-111111111111');
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', '/experiments/11111111-1111-1111-1111-111111111111');
    expect(link).toHaveTextContent('homepage_recs_v2');
  });

  it('shows "No running experiments" when none exist', async () => {
    // Override to return only DRAFT experiments
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json({
          experiments: [
            {
              experimentId: 'draft-only',
              name: 'draft_experiment',
              description: '',
              ownerEmail: 'test@streamco.com',
              type: 'AB',
              state: 'DRAFT',
              variants: [],
              layerId: 'layer-test',
              hashSalt: 'salt-test',
              primaryMetricId: 'metric1',
              secondaryMetricIds: [],
              guardrailConfigs: [],
              guardrailAction: 'AUTO_PAUSE',
              isCumulativeHoldout: false,
              createdAt: '2026-03-01T00:00:00Z',
            },
          ],
          nextPageToken: '',
        });
      }),
    );

    render(<MonitoringPage />);
    await waitFor(() => {
      expect(screen.getByTestId('no-running-experiments')).toBeInTheDocument();
    });

    expect(screen.getByText('No running experiments.')).toBeInTheDocument();
  });

  it('days running calculation is correct', async () => {
    // Override with a controlled experiment startedAt
    const twoDaysAgo = new Date(Date.now() - 2 * 24 * 60 * 60 * 1000).toISOString();
    server.use(
      http.post(`${MGMT_SVC}/ListExperiments`, () => {
        return HttpResponse.json({
          experiments: [
            {
              experimentId: 'test-days',
              name: 'days_test',
              description: '',
              ownerEmail: 'test@streamco.com',
              type: 'AB',
              state: 'RUNNING',
              variants: [],
              layerId: 'layer-test',
              hashSalt: 'salt-test',
              primaryMetricId: 'metric1',
              secondaryMetricIds: [],
              guardrailConfigs: [],
              guardrailAction: 'AUTO_PAUSE',
              isCumulativeHoldout: false,
              createdAt: '2026-03-01T00:00:00Z',
              startedAt: twoDaysAgo,
            },
          ],
          nextPageToken: '',
        });
      }),
      // Return empty guardrail status for this experiment
      http.post(`${MGMT_SVC}/GetGuardrailStatus`, () => {
        return HttpResponse.json({
          experimentId: 'test-days',
          breaches: [],
          isPaused: false,
        });
      }),
      // Return 404 for analysis (no data yet)
      http.post(`${ANALYSIS_SVC}/GetAnalysisResult`, () => {
        return HttpResponse.json(
          { error: 'No analysis result' },
          { status: 404 },
        );
      }),
    );

    render(<MonitoringPage />);
    await waitFor(() => {
      expect(screen.getByTestId('days-running-test-days')).toBeInTheDocument();
    });

    expect(screen.getByTestId('days-running-test-days')).toHaveTextContent('2');
  });
});
