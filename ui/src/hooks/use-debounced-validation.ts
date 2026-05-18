import { useEffect, useState } from 'react';
import type { StepValidation } from '../lib/validation';

export type DebouncedValidationStatus = 'idle' | 'pending' | 'valid' | 'invalid';

export interface DebouncedValidationResult {
  status: DebouncedValidationStatus;
  error?: string;
}

/**
 * Debounce a value, then run an (async) validator and surface the result.
 * Used by the metric creation form (ADR-026 Phase 1) to throttle re-validation
 * as the user types into FILTERED_MEAN / COMPOSITE / WINDOWED_COUNT fields.
 *
 * Status semantics:
 * - `'idle'`: initial state, no validator outcome yet for the current value.
 * - `'pending'`: debounce timer has not elapsed; a validator run is queued.
 * - `'valid'` / `'invalid'`: most recent validator outcome (plus `error` when invalid).
 *
 * Synchronous validators (the common case) are wrapped in `Promise.resolve`
 * so this hook can host both sync and async validators with one signature.
 */
export function useDebouncedValidation<T>(
  value: T,
  validator: (v: T) => StepValidation | Promise<StepValidation>,
  delayMs: number = 500,
): DebouncedValidationResult {
  const [result, setResult] = useState<DebouncedValidationResult>({ status: 'idle' });

  useEffect(() => {
    setResult({ status: 'pending' });
    let cancelled = false;
    const timer = setTimeout(async () => {
      const outcome = await Promise.resolve(validator(value));
      if (cancelled) return;
      setResult({
        status: outcome.valid ? 'valid' : 'invalid',
        error: outcome.error,
      });
    }, delayMs);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [value, delayMs, validator]);

  return result;
}
