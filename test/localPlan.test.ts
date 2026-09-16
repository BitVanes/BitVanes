import { describe, expect, it } from 'vitest';
import { buildLocalPlan } from '../src/pipeline/localPlan';
import { TreeSitterResolver } from '../src/ast/TreeSitterResolver';
import type { DiffFile } from '../src/git/pure';

const ROOT = '/ws';

const RUST = `mod util;

pub fn deposit(vault: &mut Vault, amount: u64) {
    require(amount > 0, "zero");
    let before = vault.balance;
    vault.balance += amount;
    vault.touch();
}

pub struct Vault {
    pub balance: u64,
}
`;

function diff(hunks: DiffFile['hunks']): DiffFile[] {
  return [{ path: 'src/vault.rs', absPath: `${ROOT}/src/vault.rs`, status: 'M', hunks }];
}

describe('buildLocalPlan', () => {
  const resolver = new TreeSitterResolver('/nonexistent-resources');

  it('builds ordered steps with variables and scopes from hunk lines', async () => {
    const plan = await buildLocalPlan(
      diff([{ oldStart: 4, oldCount: 2, newStart: 4, newCount: 3 }]),
      resolver,
      async () => RUST,
      ROOT,
      12,
    );
    expect(plan.totalSteps).toBeGreaterThan(1);
    expect(plan.entryPoint).toBe('src/vault.rs');
    const titles = plan.steps.map((s) => s.title);
    expect(titles.some((t) => /Validate/i.test(t))).toBe(true);
    expect(titles.some((t) => /Init before|Init vault/i.test(t))).toBe(true);
    expect(titles.some((t) => /Mutate vault\.balance|Mutate balance/i.test(t))).toBe(true);
    for (const step of plan.steps) {
      expect(step.scopeRange).toBeDefined();
      expect(step.scopeRange!.startLine).toBe(3);
      expect(step.explanation.length).toBeGreaterThan(10);
    }
    const validate = plan.steps.find((s) => /Validate/.test(s.title));
    expect(validate?.securityNote).toBeDefined();
  });

  it('caps steps at maxSteps', async () => {
    const plan = await buildLocalPlan(
      diff([{ oldStart: 1, oldCount: 10, newStart: 1, newCount: 10 }]),
      resolver,
      async () => RUST,
      ROOT,
      2,
    );
    expect(plan.steps.length).toBeLessThanOrEqual(2);
    expect(plan.steps.map((s) => s.stepIndex)).toEqual(plan.steps.map((_, i) => i));
  });

  it('throws when nothing analyzable intersects the hunks', async () => {
    await expect(
      buildLocalPlan(diff([{ oldStart: 12, oldCount: 1, newStart: 12, newCount: 1 }]), resolver, async () => RUST, ROOT, 12),
    ).rejects.toThrow(/Instant mode found no analyzable statements/);
  });
});
