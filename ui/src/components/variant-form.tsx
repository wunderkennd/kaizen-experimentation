'use client';

import { useState, useCallback } from 'react';
import type { ExperimentType, Variant } from '@/lib/types';
import { formatPercent } from '@/lib/utils';
import {
  validateVariants,
  validateJsonPayload,
  getMinVariants,
  generateVariantId,
  type VariantError,
} from '@/lib/validation';

interface VariantFormProps {
  variants: Variant[];
  experimentType: ExperimentType;
  onSave: (variants: Variant[]) => Promise<void>;
}

export function VariantForm({ variants: initialVariants, experimentType, onSave }: VariantFormProps) {
  const [variants, setVariants] = useState<Variant[]>(initialVariants);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [fieldErrors, setFieldErrors] = useState<VariantError[]>([]);
  const [bannerError, setBannerError] = useState<string | undefined>();

  const minVariants = getMinVariants(experimentType);

  const updateVariant = useCallback((index: number, field: keyof Variant, value: string | number | boolean) => {
    setVariants((prev) => {
      const next = [...prev];
      if (field === 'isControl' && value === true) {
        // Radio behavior: only one control
        next.forEach((v, i) => {
          next[i] = { ...v, isControl: i === index };
        });
      } else {
        next[index] = { ...next[index], [field]: value };
      }
      return next;
    });
    setDirty(true);
    // Clear field-level error on edit
    setFieldErrors((prev) => prev.filter((e) => !(e.index === index && e.field === field)));
  }, []);

  const handleBlur = useCallback((index: number, field: 'name' | 'trafficFraction' | 'payloadJson') => {
    const v = variants[index];
    const newErrors: VariantError[] = [];

    if (field === 'name' && !v.name.trim()) {
      newErrors.push({ index, field: 'name', message: 'Variant name is required' });
    }
    if (field === 'trafficFraction' && (v.trafficFraction < 0 || v.trafficFraction > 1)) {
      newErrors.push({ index, field: 'trafficFraction', message: 'Traffic must be between 0 and 1' });
    }
    if (field === 'payloadJson' && !validateJsonPayload(v.payloadJson)) {
      newErrors.push({ index, field: 'payloadJson', message: 'Invalid JSON' });
    }

    setFieldErrors((prev) => {
      const without = prev.filter((e) => !(e.index === index && e.field === field));
      return [...without, ...newErrors];
    });
  }, [variants]);

  const addVariant = useCallback(() => {
    setVariants((prev) => [
      ...prev,
      {
        variantId: generateVariantId(),
        name: '',
        trafficFraction: 0,
        isControl: false,
        payloadJson: '{}',
      },
    ]);
    setDirty(true);
  }, []);

  const removeVariant = useCallback((index: number) => {
    setVariants((prev) => prev.filter((_, i) => i !== index));
    setFieldErrors((prev) =>
      prev
        .filter((e) => e.index !== index)
        .map((e) => (e.index > index ? { ...e, index: e.index - 1 } : e)),
    );
    setDirty(true);
  }, []);

  const distributeTraffic = useCallback(() => {
    setVariants((prev) => {
      const count = prev.length;
      if (count === 0) return prev;
      const equalTraffic = Math.floor((1.0 / count) * 1000) / 1000;
      const remainder = Math.round((1.0 - equalTraffic * count) * 1000) / 1000;

      return prev.map((v, i) => ({
        ...v,
        trafficFraction: i === count - 1 ? Math.round((equalTraffic + remainder) * 1000) / 1000 : equalTraffic,
      }));
    });
    setDirty(true);
    setBannerError(undefined);
    setFieldErrors((prev) => prev.filter((e) => e.field !== 'trafficFraction'));
  }, []);

  const handleSave = async () => {
    const result = validateVariants(variants, experimentType);
    setFieldErrors(result.errors);
    setBannerError(result.bannerError);

    if (!result.valid) return;

    setSaving(true);
    try {
      await onSave(variants);
      setDirty(false);
    } finally {
      setSaving(false);
    }
  };

  const trafficSum = variants.reduce((acc, v) => acc + v.trafficFraction, 0);
  const trafficSumValid = Math.abs(trafficSum - 1.0) < 1e-9;

  const getError = (index: number, field: string): string | undefined =>
    fieldErrors.find((e) => e.index === index && e.field === field)?.message;

  const hasErrors = fieldErrors.length > 0 || !!bannerError;

  return (
    <div>
      {bannerError && (
        <div className="mb-4 rounded-md bg-red-50 p-3 text-sm text-red-700" role="alert">
          {bannerError}
        </div>
      )}

      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Name</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Traffic</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Control</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">Payload JSON</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500" />
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200 bg-white">
            {variants.map((v, i) => (
              <tr key={v.variantId}>
                <td className="px-4 py-2">
                  <input
                    type="text"
                    value={v.name}
                    onChange={(e) => updateVariant(i, 'name', e.target.value)}
                    onBlur={() => handleBlur(i, 'name')}
                    aria-label={`Variant ${i + 1} name`}
                    aria-required="true"
                    aria-invalid={!!getError(i, 'name')}
                    aria-describedby={getError(i, 'name') ? `variant-${i}-name-error` : undefined}
                    className={`w-full rounded border px-2 py-1 text-sm ${
                      getError(i, 'name') ? 'border-red-500' : 'border-gray-300'
                    }`}
                  />
                  {getError(i, 'name') && (
                    <p id={`variant-${i}-name-error`} className="mt-1 text-xs text-red-600">{getError(i, 'name')}</p>
                  )}
                </td>
                <td className="px-4 py-2">
                  <input
                    type="number"
                    min={0}
                    max={1}
                    step={0.01}
                    value={v.trafficFraction}
                    onChange={(e) => updateVariant(i, 'trafficFraction', parseFloat(e.target.value) || 0)}
                    onBlur={() => handleBlur(i, 'trafficFraction')}
                    aria-label={`Variant ${i + 1} traffic`}
                    aria-invalid={!!getError(i, 'trafficFraction')}
                    aria-describedby={getError(i, 'trafficFraction') ? `variant-${i}-traffic-error` : undefined}
                    className={`w-24 rounded border px-2 py-1 text-sm ${
                      getError(i, 'trafficFraction') ? 'border-red-500' : 'border-gray-300'
                    }`}
                  />
                  {getError(i, 'trafficFraction') && (
                    <p id={`variant-${i}-traffic-error`} className="mt-1 text-xs text-red-600">{getError(i, 'trafficFraction')}</p>
                  )}
                </td>
                <td className="px-4 py-2 text-center">
                  <input
                    type="radio"
                    name="control-variant"
                    checked={v.isControl}
                    onChange={() => updateVariant(i, 'isControl', true)}
                    aria-label={`Set ${v.name || `variant ${i + 1}`} as control`}
                  />
                </td>
                <td className="px-4 py-2">
                  <textarea
                    value={v.payloadJson}
                    onChange={(e) => updateVariant(i, 'payloadJson', e.target.value)}
                    onBlur={() => handleBlur(i, 'payloadJson')}
                    aria-label={`Variant ${i + 1} payload`}
                    aria-invalid={!!getError(i, 'payloadJson')}
                    aria-describedby={getError(i, 'payloadJson') ? `variant-${i}-payload-error` : undefined}
                    rows={2}
                    className={`w-full rounded border px-2 py-1 font-mono text-xs ${
                      getError(i, 'payloadJson') ? 'border-red-500' : 'border-gray-300'
                    }`}
                  />
                  {getError(i, 'payloadJson') && (
                    <p id={`variant-${i}-payload-error`} className="mt-1 text-xs text-red-600">{getError(i, 'payloadJson')}</p>
                  )}
                </td>
                <td className="px-4 py-2">
                  <button
                    type="button"
                    onClick={() => removeVariant(i)}
                    disabled={variants.length <= minVariants}
                    aria-label={`Remove variant ${v.name || i + 1}`}
                    className="text-sm text-red-600 hover:text-red-800 disabled:cursor-not-allowed disabled:text-gray-400"
                  >
                    Remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Traffic sum indicator */}
      <div
        role="status"
        aria-live="polite"
        className={`mt-2 text-sm font-medium ${trafficSumValid ? 'text-green-700' : 'text-red-700'}`}
      >
        Total traffic: {formatPercent(trafficSum)}
      </div>

      {/* Actions */}
      <div className="mt-4 flex items-center gap-3">
        <button
          type="button"
          onClick={addVariant}
          className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
        >
          Add Variant
        </button>
        <button
          type="button"
          onClick={distributeTraffic}
          className="rounded-md border border-gray-300 bg-white px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50"
          aria-label="Distribute traffic evenly across all variants"
        >
          Distribute Evenly
        </button>
        <button
          type="button"
          onClick={handleSave}
          disabled={!dirty || saving || hasErrors}
          className="rounded-md bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {saving ? 'Saving...' : 'Save Variants'}
        </button>
      </div>
    </div>
  );
}
