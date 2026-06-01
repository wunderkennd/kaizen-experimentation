/**
 * ADR-026 Phase 3 / Task D2 (Lock L5).
 *
 * Verifies the M6 UI surfaces a deprecation toast when an operator creates a
 * CUSTOM-typed metric, and stays silent for non-deprecated types.
 *
 * Why this test file (and not page.test.tsx co-located): the repo convention
 * (verified across ~30 sibling test files) is `ui/src/__tests__/*.test.tsx`,
 * not co-located tests.
 *
 * Scope-adjusted from the original plan: the plan called for reading the
 * M5 trailer `x-kaizen-deprecation` via a Connect-Web interceptor; `lib/api.ts`
 * uses raw `fetch` (no Connect interceptor) and reading HTTP trailers via
 * `fetch` is not portable across browsers. The UI instead reads the typed
 * `MetricDefinition.type` echoed by the server — same outcome, no transport
 * coupling. The on-wire trailer remains for non-UI consumers (CLI, automation,
 * telemetry).
 */
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { server } from '@/__mocks__/server';
import NewMetricPage from '@/app/metrics/new/page';
// ADR-026 Phase 3 / Task D2: the L5-locked message and the per-type gate
// live in `@/lib/metric-deprecation` (extracted out of `page.tsx` so they
// can be imported without colliding with Next.js App Router's strict
// page-export allowlist).
import {
  DEPRECATION_TOAST_MESSAGE,
  shouldShowCustomDeprecationToast,
} from '@/lib/metric-deprecation';
import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';
import type { MetricType } from '@/lib/types';

const MGMT_SVC = '*/experimentation.management.v1.ExperimentManagementService';

const experimenterUser: AuthUser = { email: 'test@streamco.com', role: 'experimenter' };

const mockPush = vi.fn();
const mockAddToast = vi.fn();

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: mockPush, back: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
  usePathname: () => '/metrics/new',
}));

vi.mock('@/lib/toast-context', () => ({
  useToast: () => ({ addToast: mockAddToast, removeToast: vi.fn(), toasts: [] }),
  ToastProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

beforeEach(() => {
  mockPush.mockClear();
  mockAddToast.mockClear();
});

function renderPage() {
  return render(
    <AuthProvider initialUser={experimenterUser}>
      <NewMetricPage />
    </AuthProvider>,
  );
}

/**
 * Stub `CreateMetricDefinition` to echo whatever `type` we want the server
 * to confirm. The page reads `created.type` (server-echoed) to decide whether
 * to surface the deprecation toast, so this is the lever that toggles the
 * UI branch under test.
 *
 * Mirrors the wire-format convention used by the live M5 handler: enum values
 * use the `METRIC_TYPE_` prefix (the api.ts adapter strips it).
 */
function stubCreateResponseWithType(type: string) {
  server.use(
    http.post(`${MGMT_SVC}/CreateMetricDefinition`, () =>
      HttpResponse.json({
        metricId: 'new_metric',
        name: 'New Metric',
        description: '',
        type: `METRIC_TYPE_${type}`,
        sourceEventType: '',
        stakeholder: 'METRIC_STAKEHOLDER_USER',
        aggregationLevel: 'METRIC_AGGREGATION_LEVEL_USER',
      }),
    ),
  );
}

/** Fill the minimal form fields the page requires for a legacy-type submit. */
async function fillBasicFields(user: ReturnType<typeof userEvent.setup>, metricType: MetricType) {
  await user.type(screen.getByTestId('metric-id-input'), 'new_metric');
  await user.type(screen.getByTestId('metric-name-input'), 'New Metric');
  // Type select — the page wipes the per-type config on type change, and for
  // legacy types like CUSTOM / MEAN the form passes the server-validation gate
  // with no extra fields (the form preview shows a "legacy types out of scope"
  // note, but the submit button still enables once basics + type are present).
  await user.selectOptions(screen.getByTestId('metric-type-select'), metricType);
}

describe('shouldShowCustomDeprecationToast (ADR-026 Phase 3 / D2)', () => {
  it('returns true for CUSTOM', () => {
    expect(shouldShowCustomDeprecationToast({ type: 'CUSTOM' })).toBe(true);
  });

  it.each<MetricType>([
    'MEAN',
    'PROPORTION',
    'RATIO',
    'COUNT',
    'PERCENTILE',
    'FILTERED_MEAN',
    'COMPOSITE',
    'WINDOWED_COUNT',
    'METRICQL',
  ])('returns false for %s', (type) => {
    expect(shouldShowCustomDeprecationToast({ type })).toBe(false);
  });
});

describe('DEPRECATION_TOAST_MESSAGE (L5 lock)', () => {
  it('matches the L5-locked operator-facing string exactly', () => {
    // If this assertion changes, also update:
    //   - docs/runbooks/m5-metric-definitions.md#custom-deprecation
    //   - the x-kaizen-deprecation trailer string in M5
    //   - the plan at docs/superpowers/plans/2026-05-30-adr-026-phase-3-custom-migration.md (L5)
    expect(DEPRECATION_TOAST_MESSAGE).toBe(
      'Custom SQL metrics are deprecated. Use MetricQL or structured types instead. See docs/runbooks/m5-metric-definitions.md#custom-deprecation.',
    );
  });

  it('starts with the locked opener "Custom SQL metrics are deprecated."', () => {
    // Defends against drift on the prefix even if the suffix evolves.
    expect(DEPRECATION_TOAST_MESSAGE).toMatch(/^Custom SQL metrics are deprecated\./);
  });
});

describe('NewMetricPage deprecation toast (ADR-026 Phase 3 / D2)', () => {
  it('emits a warning toast with the L5 message when a CUSTOM metric is created', async () => {
    stubCreateResponseWithType('CUSTOM');
    const user = userEvent.setup();
    renderPage();

    await fillBasicFields(user, 'CUSTOM');
    await user.click(screen.getByTestId('metric-submit-button'));

    await waitFor(() => {
      expect(mockAddToast).toHaveBeenCalledWith(DEPRECATION_TOAST_MESSAGE, 'warning');
    });
    expect(mockAddToast).toHaveBeenCalledTimes(1);

    // Sanity: navigation still happens after the toast queues.
    await waitFor(() => {
      expect(mockPush).toHaveBeenCalledWith(expect.stringMatching(/^\/metrics\?created=/));
    });
  });

  it('does NOT emit a deprecation toast when a MEAN metric is created', async () => {
    stubCreateResponseWithType('MEAN');
    const user = userEvent.setup();
    renderPage();

    await fillBasicFields(user, 'MEAN');
    await user.click(screen.getByTestId('metric-submit-button'));

    // Wait for the create to land before asserting silence — otherwise we'd
    // pass trivially by checking before the RPC resolves.
    await waitFor(() => {
      expect(mockPush).toHaveBeenCalledWith(expect.stringMatching(/^\/metrics\?created=/));
    });
    expect(mockAddToast).not.toHaveBeenCalled();
  });

  it('does NOT emit a deprecation toast when a FILTERED_MEAN metric is created', async () => {
    // FILTERED_MEAN normally needs a per-type config to enable submit, but the
    // server-echoed type is what gates the toast, so we drive the form as
    // MEAN (passes client validation) and let the server stub echo back
    // FILTERED_MEAN. This isolates the toast-decision branch from the
    // unrelated client-side validation flow.
    stubCreateResponseWithType('FILTERED_MEAN');
    const user = userEvent.setup();
    renderPage();

    await fillBasicFields(user, 'MEAN');
    await user.click(screen.getByTestId('metric-submit-button'));

    await waitFor(() => {
      expect(mockPush).toHaveBeenCalledWith(expect.stringMatching(/^\/metrics\?created=/));
    });
    expect(mockAddToast).not.toHaveBeenCalled();
  });
});
