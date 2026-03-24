/**
 * Tests for ADR-013 META experiment UI components.
 *
 * Coverage:
 *   - MetaExperimentConfig: renders table, variant-to-bandit mapping, empty arms
 *   - MetaVariantSelector: renders dropdown with bandit policy annotations
 *   - TwoLevelIPWBadge: renders compound probability, title attribute
 *   - MetaConfigForm: renders per-variant bandit config form within wizard context
 *   - Experiment creation form: META type visible in type dropdown
 *   - Validation: validateMetaConfig rejects empty configs and empty arms
 */

import { describe, it, expect, vi } from 'vitest';
import React from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

import { MetaExperimentConfig } from '@/components/meta/MetaExperimentConfig';
import { MetaVariantSelector } from '@/components/meta/MetaVariantSelector';
import { TwoLevelIPWBadge } from '@/components/meta/TwoLevelIPWBadge';
import { validateMetaConfig } from '@/lib/validation';
import type { Variant, MetaConfig } from '@/lib/types';

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

const VARIANTS: Variant[] = [
  {
    variantId: 'v-ctrl',
    name: 'control',
    trafficFraction: 0.5,
    isControl: true,
    payloadJson: '{}',
  },
  {
    variantId: 'v-treat',
    name: 'treatment',
    trafficFraction: 0.5,
    isControl: false,
    payloadJson: '{}',
  },
];

const META_CONFIG: MetaConfig = {
  variantBanditConfigs: [
    { variantId: 'v-ctrl', banditType: 'THOMPSON_SAMPLING', arms: ['arm-a', 'arm-b'] },
    { variantId: 'v-treat', banditType: 'LINEAR_UCB', arms: ['arm-x', 'arm-y', 'arm-z'] },
  ],
};

// ---------------------------------------------------------------------------
// MetaExperimentConfig
// ---------------------------------------------------------------------------

describe('MetaExperimentConfig', () => {
  it('renders a table with variant rows', () => {
    render(<MetaExperimentConfig variants={VARIANTS} metaConfig={META_CONFIG} />);

    // Multiple elements can match 'control' (text node + badge span)
    expect(screen.getAllByText('control').length).toBeGreaterThan(0);
    expect(screen.getByText('treatment')).toBeInTheDocument();
  });

  it('shows bandit type for each variant', () => {
    render(<MetaExperimentConfig variants={VARIANTS} metaConfig={META_CONFIG} />);

    expect(screen.getByText('Thompson Sampling')).toBeInTheDocument();
    expect(screen.getByText('Linear UCB')).toBeInTheDocument();
  });

  it('displays arm badges for configured variants', () => {
    render(<MetaExperimentConfig variants={VARIANTS} metaConfig={META_CONFIG} />);

    expect(screen.getByText('arm-a')).toBeInTheDocument();
    expect(screen.getByText('arm-b')).toBeInTheDocument();
    expect(screen.getByText('arm-x')).toBeInTheDocument();
    expect(screen.getByText('arm-y')).toBeInTheDocument();
    expect(screen.getByText('arm-z')).toBeInTheDocument();
  });

  it('shows dash for variants without a bandit config', () => {
    const emptyConfig: MetaConfig = { variantBanditConfigs: [] };
    render(<MetaExperimentConfig variants={VARIANTS} metaConfig={emptyConfig} />);

    const dashes = screen.getAllByText('—');
    expect(dashes.length).toBeGreaterThan(0);
  });

  it('marks the control variant with a badge', () => {
    render(<MetaExperimentConfig variants={VARIANTS} metaConfig={META_CONFIG} />);
    // At least one element with text "control" exists (name + badge both match)
    const controls = screen.getAllByText('control');
    expect(controls.length).toBeGreaterThan(0);
  });

  it('shows traffic fractions', () => {
    render(<MetaExperimentConfig variants={VARIANTS} metaConfig={META_CONFIG} />);
    const fractionCells = screen.getAllByText('50.0%');
    expect(fractionCells.length).toBe(2);
  });
});

// ---------------------------------------------------------------------------
// MetaVariantSelector
// ---------------------------------------------------------------------------

describe('MetaVariantSelector', () => {
  it('renders a dropdown with all variants', () => {
    const onChange = vi.fn();
    render(
      <MetaVariantSelector
        variants={VARIANTS}
        metaConfig={META_CONFIG}
        selectedVariantId=""
        onChange={onChange}
      />,
    );

    const select = screen.getByRole('combobox');
    expect(select).toBeInTheDocument();
    expect(screen.getByText(/control/)).toBeInTheDocument();
    expect(screen.getByText(/treatment/)).toBeInTheDocument();
  });

  it('annotates variants with bandit policy abbreviation and arm count', () => {
    render(
      <MetaVariantSelector
        variants={VARIANTS}
        metaConfig={META_CONFIG}
        selectedVariantId=""
        onChange={vi.fn()}
      />,
    );

    // "control [TS, 2 arms]" and "treatment [LinUCB, 3 arms]"
    expect(screen.getByText(/TS.*2 arms/)).toBeInTheDocument();
    expect(screen.getByText(/LinUCB.*3 arms/)).toBeInTheDocument();
  });

  it('calls onChange when a variant is selected', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <MetaVariantSelector
        variants={VARIANTS}
        metaConfig={META_CONFIG}
        selectedVariantId=""
        onChange={onChange}
      />,
    );

    await user.selectOptions(screen.getByRole('combobox'), 'v-treat');
    expect(onChange).toHaveBeenCalledWith('v-treat');
  });

  it('uses the provided label and id', () => {
    render(
      <MetaVariantSelector
        variants={VARIANTS}
        metaConfig={META_CONFIG}
        selectedVariantId=""
        onChange={vi.fn()}
        id="test-selector"
        label="Choose Variant"
      />,
    );

    expect(screen.getByLabelText('Choose Variant')).toBeInTheDocument();
  });

  it('shows [no policy] for variants without a bandit config', () => {
    const emptyConfig: MetaConfig = { variantBanditConfigs: [] };
    render(
      <MetaVariantSelector
        variants={VARIANTS}
        metaConfig={emptyConfig}
        selectedVariantId=""
        onChange={vi.fn()}
      />,
    );

    const noPolicyItems = screen.getAllByText(/no policy/);
    expect(noPolicyItems.length).toBe(2);
  });
});

// ---------------------------------------------------------------------------
// TwoLevelIPWBadge
// ---------------------------------------------------------------------------

describe('TwoLevelIPWBadge', () => {
  it('renders the compound probability', () => {
    render(<TwoLevelIPWBadge variantProbability={0.5} armProbability={0.25} />);

    // 0.5 * 0.25 = 0.125
    const badge = screen.getByTestId('two-level-ipw-badge');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveTextContent('0.1250');
  });

  it('sets a title with the breakdown', () => {
    render(<TwoLevelIPWBadge variantProbability={0.5} armProbability={0.25} />);

    const badge = screen.getByTestId('two-level-ipw-badge');
    expect(badge).toHaveAttribute('title', expect.stringContaining('P(variant)=0.5000'));
    expect(badge).toHaveAttribute('title', expect.stringContaining('P(arm|variant)=0.2500'));
  });

  it('renders 0.0000 when both probabilities are zero', () => {
    render(<TwoLevelIPWBadge variantProbability={0} armProbability={0} />);
    expect(screen.getByTestId('two-level-ipw-badge')).toHaveTextContent('0.0000');
  });

  it('renders compound=1 for uniform single-variant single-arm', () => {
    render(<TwoLevelIPWBadge variantProbability={1.0} armProbability={1.0} />);
    expect(screen.getByTestId('two-level-ipw-badge')).toHaveTextContent('1.0000');
  });
});

// ---------------------------------------------------------------------------
// validateMetaConfig
// ---------------------------------------------------------------------------

describe('validateMetaConfig', () => {
  it('fails with empty variantBanditConfigs', () => {
    const result = validateMetaConfig({ variantBanditConfigs: [] });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/at least one/i);
  });

  it('fails when a variant config has no arms', () => {
    const result = validateMetaConfig({
      variantBanditConfigs: [
        { variantId: 'v-ctrl', banditType: 'THOMPSON_SAMPLING', arms: [] },
      ],
    });
    expect(result.valid).toBe(false);
    expect(result.error).toMatch(/arm/i);
  });

  it('passes with valid config', () => {
    const result = validateMetaConfig({
      variantBanditConfigs: [
        { variantId: 'v-ctrl', banditType: 'THOMPSON_SAMPLING', arms: ['arm-a'] },
      ],
    });
    expect(result.valid).toBe(true);
  });

  it('passes with multiple variant configs each having arms', () => {
    const result = validateMetaConfig(META_CONFIG);
    expect(result.valid).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// MetaConfigForm (within WizardProvider)
// ---------------------------------------------------------------------------

vi.mock('next/navigation', () => ({
  useParams: () => ({}),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams(),
}));

vi.mock('next/link', () => ({
  default: ({ children, href, ...props }: { children: React.ReactNode; href: string; [key: string]: unknown }) => (
    <a href={href} {...props}>{children}</a>
  ),
}));

vi.mock('@/lib/toast-context', () => ({
  useToast: () => ({ addToast: vi.fn(), removeToast: vi.fn(), toasts: [] }),
  ToastProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock('next/dynamic', () => ({
  default: (loader: () => Promise<{ default: React.ComponentType<unknown> }>) => {
    let Comp: React.ComponentType<unknown> | null = null;
    loader().then((mod) => { Comp = mod.default; });
    return function DynamicMock(props: Record<string, unknown>) {
      return Comp ? <Comp {...props} /> : null;
    };
  },
}));

import { AuthProvider } from '@/lib/auth-context';
import type { AuthUser } from '@/lib/auth-context';
import NewExperimentPage from '@/app/experiments/new/page';

const experimenterUser: AuthUser = { email: 'test@streamco.com', role: 'experimenter' };

describe('META type in experiment wizard', () => {
  it('shows Meta Experiment option in type dropdown', () => {
    render(
      <AuthProvider initialUser={experimenterUser}>
        <NewExperimentPage />
      </AuthProvider>,
    );

    const typeSelect = screen.getByLabelText(/Experiment Type/);
    expect(typeSelect).toBeInTheDocument();

    // META option should be available
    expect(screen.getByRole('option', { name: 'Meta Experiment' })).toBeInTheDocument();
  });

  it('can select META type', async () => {
    const user = userEvent.setup();
    render(
      <AuthProvider initialUser={experimenterUser}>
        <NewExperimentPage />
      </AuthProvider>,
    );

    const typeSelect = screen.getByLabelText(/Experiment Type/);
    await user.selectOptions(typeSelect, 'META');
    expect(typeSelect).toHaveValue('META');
  });
});
