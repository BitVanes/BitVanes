import { describe, expect, it } from 'vitest';
import { PlanValidationError, coerceRange, validateWalkthroughPlan } from '../src/types/protocol';

const baseStep = {
  stepIndex: 0,
  title: 'Validate input',
  filePath: 'src/vault.rs',
  range: { startLine: 3, startCol: 5, endLine: 3, endCol: 40 },
  explanation: 'Checks the amount is positive before any state changes.',
};

describe('coerceRange', () => {
  it('accepts valid 1-based ranges', () => {
    expect(coerceRange({ startLine: 1, startCol: 1, endLine: 2, endCol: 10 }, [], 'r')).toEqual({
      startLine: 1,
      startCol: 1,
      endLine: 2,
      endCol: 10,
    });
  });

  it('normalizes 0-based lines to 1-based without shifting columns', () => {
    const issues: Array<{ field: string; message: string }> = [];
    const r = coerceRange({ startLine: 0, startCol: 4, endLine: 5, endCol: 20 }, issues, 'r');
    expect(r).toEqual({ startLine: 1, startCol: 4, endLine: 6, endCol: 20 });
    expect(issues.length).toBe(1);
  });

  it('keeps 1-based lines intact when only columns are 0-based', () => {
    const issues: Array<{ field: string; message: string }> = [];
    // A common model confusion: 1-based lines with 0-based columns. The line
    // numbers must NOT shift.
    const r = coerceRange({ startLine: 5, startCol: 0, endLine: 7, endCol: 12 }, issues, 'r');
    expect(r).toEqual({ startLine: 5, startCol: 1, endLine: 7, endCol: 12 });
    expect(issues.some((i) => /0-based column/.test(i.message))).toBe(true);
  });

  it('treats an omitted endCol as end-of-line', () => {
    const r = coerceRange({ startLine: 3, startCol: 1, endLine: 8 }, [], 'r');
    expect(r?.endCol).toBe(100_000);
  });

  it('repairs a nonsense endLine of 0 without inventing lines', () => {
    const r = coerceRange({ startLine: 10, startCol: 2, endLine: 0, endCol: 5 }, [], 'r');
    expect(r?.startLine).toBe(10);
    expect(r?.endLine).toBe(10);
  });

  it('swaps inverted line ranges', () => {
    const r = coerceRange({ startLine: 9, startCol: 1, endLine: 4, endCol: 2 }, [], 'r');
    expect(r?.startLine).toBe(4);
    expect(r?.endLine).toBe(9);
  });

  it('rejects non-numeric lines', () => {
    expect(coerceRange({ startLine: 'x', startCol: 1, endLine: 2, endCol: 2 }, [], 'r')).toBeUndefined();
  });
});

describe('validateWalkthroughPlan', () => {
  it('validates a well-formed plan and renumbers steps', () => {
    const plan = validateWalkthroughPlan(
      {
        summary: 'Vault deposit flow',
        entryPoint: 'src/vault.rs',
        totalSteps: 99,
        steps: [
          baseStep,
          {
            stepIndex: 42,
            title: 'Mutate balance',
            filePath: 'src\\vault.rs',
            range: { startLine: 10, startCol: 1, endLine: 10, endCol: 30 },
            variable: { name: 'balance', action: 'updated', stateBefore: '100', stateAfter: '95' },
            explanation: 'Debits the sender.',
          },
        ],
      },
      { maxSteps: 12 },
    );
    expect(plan.totalSteps).toBe(2);
    expect(plan.steps.map((s) => s.stepIndex)).toEqual([0, 1]);
    expect(plan.steps[1].filePath).toBe('src/vault.rs');
    expect(plan.steps[1].variable?.action).toBe('mutate');
  });

  it('drops steps without explanations and invalid ranges', () => {
    const plan = validateWalkthroughPlan(
      {
        steps: [
          baseStep,
          { ...baseStep, explanation: '' },
          { ...baseStep, range: 'nope' },
        ],
      },
      { maxSteps: 12, defaultFilePath: 'src/vault.rs' },
    );
    expect(plan.steps.length).toBe(1);
  });

  it('caps steps at maxSteps', () => {
    const steps = Array.from({ length: 20 }, (_, i) => ({ ...baseStep, stepIndex: i }));
    const plan = validateWalkthroughPlan({ steps }, { maxSteps: 5 });
    expect(plan.steps.length).toBe(5);
  });

  it('applies the default filePath when missing', () => {
    const plan = validateWalkthroughPlan(
      { steps: [{ ...baseStep, filePath: undefined }] },
      { maxSteps: 12, defaultFilePath: 'fallback.rs' },
    );
    expect(plan.steps[0].filePath).toBe('fallback.rs');
  });

  it('throws when nothing survives validation', () => {
    expect(() => validateWalkthroughPlan({ steps: [] }, { maxSteps: 12 })).toThrow(PlanValidationError);
    expect(() => validateWalkthroughPlan('nonsense', { maxSteps: 12 })).toThrow(PlanValidationError);
  });
});
