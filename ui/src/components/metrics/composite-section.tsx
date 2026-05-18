'use client';

import type { CompositeConfig } from '@/lib/types';
import { CompositeOperator } from '@/lib/types';
import { validateCompositeConfig } from '@/lib/validation';
import { useDebouncedValidation } from '@/hooks/use-debounced-validation';
import { OperandPicker } from './operand-picker';

interface CompositeSectionProps {
  value: CompositeConfig | undefined;
  onChange: (next: CompositeConfig) => void;
  disabled?: boolean;
}

// Numeric values match the CompositeOperator enum from types.ts.
const OPERATOR_OPTIONS: { value: CompositeOperator; label: string; arity: string }[] = [
  { value: CompositeOperator.ADD,          label: 'ADD',          arity: 'requires ≥ 2 operands' },
  { value: CompositeOperator.SUBTRACT,     label: 'SUBTRACT',     arity: 'requires exactly 2 operands' },
  { value: CompositeOperator.MULTIPLY,     label: 'MULTIPLY',     arity: 'requires ≥ 2 operands' },
  { value: CompositeOperator.DIVIDE,       label: 'DIVIDE',       arity: 'requires exactly 2 operands' },
  { value: CompositeOperator.WEIGHTED_SUM, label: 'WEIGHTED_SUM', arity: 'requires ≥ 2 operands; each weight > 0' },
];

const DEFAULT_CONFIG: CompositeConfig = { operator: CompositeOperator.UNSPECIFIED, operands: [] };

export function CompositeSection({ value, onChange, disabled }: CompositeSectionProps) {
  const cfg = value ?? DEFAULT_CONFIG;
  const validation = useDebouncedValidation(cfg, validateCompositeConfig);
  const opMeta = OPERATOR_OPTIONS.find((o) => o.value === cfg.operator);
  const showWeights = cfg.operator === CompositeOperator.WEIGHTED_SUM;

  return (
    <fieldset disabled={disabled} className="flex flex-col gap-4 rounded border border-indigo-200 bg-indigo-50/30 p-4">
      <legend className="px-2 text-sm font-semibold text-indigo-900">COMPOSITE</legend>

      <div className="flex flex-col gap-1">
        <label htmlFor="composite-operator" className="text-sm font-medium text-gray-700">Operator</label>
        <select
          id="composite-operator"
          value={cfg.operator}
          onChange={(e) => {
            const next = Number(e.target.value) as CompositeOperator;
            // Switching from WEIGHTED_SUM to a non-weighted op preserves operands but resets weights to 0.
            // Switching TO WEIGHTED_SUM defaults weights to 1.0 if previously 0.
            const operands = cfg.operands.map((op) => ({
              ...op,
              weight: next === CompositeOperator.WEIGHTED_SUM ? (op.weight > 0 ? op.weight : 1.0) : 0,
            }));
            onChange({ ...cfg, operator: next, operands });
          }}
          className="rounded border border-gray-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none"
        >
          <option value={CompositeOperator.UNSPECIFIED}>— select operator —</option>
          {OPERATOR_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>{opt.label}</option>
          ))}
        </select>
        {opMeta && <p className="text-xs text-gray-500">{opMeta.arity}</p>}
      </div>

      <div className="flex flex-col gap-1">
        <label className="text-sm font-medium text-gray-700">Operands</label>
        <OperandPicker
          value={cfg.operands}
          onChange={(next) => onChange({ ...cfg, operands: next })}
          showWeights={showWeights}
          disabled={disabled}
        />
        <p className="text-xs text-gray-500">
          Composite metrics reference other metric definitions by ID. Cycles + depth &gt; 5 are rejected server-side.
        </p>
      </div>

      {validation.status === 'invalid' && (
        <p className="text-sm text-red-700" role="alert">{validation.error}</p>
      )}
    </fieldset>
  );
}
