import { describe, expect, it } from 'vitest';
import { buildDiffRequest, buildSelectionRequest, mergeWindows, refinePlanRanges } from '../src/pipeline/contextBuilder';
import { TreeSitterResolver } from '../src/ast/TreeSitterResolver';
import { SYSTEM_PROMPT, buildUserPrompt, formatAstSummary } from '../src/llm/prompts';
import type { DiffFile } from '../src/git/pure';
import type { SelectionTarget } from '../src/git/GitProvider';

const ROOT = '/ws';

const RUST_SOURCE = `mod util;

pub fn deposit(vault: &mut Vault, amount: u64) {
    require(amount > 0, "zero");
    vault.balance += amount;
    vault.touch();
}

pub struct Vault {
    pub balance: u64,
}
`;

function diffFile(path: string, absPath: string, hunks: DiffFile['hunks']): DiffFile {
  return { path, absPath, status: 'M', hunks };
}

describe('mergeWindows', () => {
  it('merges overlapping hunks with context', () => {
    const w = mergeWindows([{ start: 10, end: 12 }, { start: 14, end: 15 }], 2, 100);
    expect(w).toEqual([{ start: 8, end: 17 }]);
  });

  it('keeps distant hunks apart', () => {
    const w = mergeWindows([{ start: 10, end: 10 }, { start: 50, end: 50 }], 3, 100);
    expect(w).toEqual([
      { start: 7, end: 13 },
      { start: 47, end: 53 },
    ]);
  });

  it('clamps to line count', () => {
    const w = mergeWindows([{ start: 8, end: 9 }], 5, 10);
    expect(w).toEqual([{ start: 3, end: 10 }]);
  });
});

describe('buildDiffRequest', () => {
  const resolver = new TreeSitterResolver('/nonexistent-resources');

  it('builds numbered snippets and AST summaries', async () => {
    const files = [diffFile('src/vault.rs', `${ROOT}/src/vault.rs`, [{ oldStart: 3, oldCount: 2, newStart: 3, newCount: 2 }])];
    const req = await buildDiffRequest(files, resolver, async () => RUST_SOURCE, 2, ROOT);
    expect(req.mode).toBe('diff');
    expect(req.language).toBe('rust');
    expect(req.files[0]!.path).toBe('src/vault.rs');
    expect(req.files[0]!.snippet).toContain('4│     require(amount > 0, "zero");');
    expect(req.files[0]!.astSummary).toMatch(/deposit|Vault/);
  });

  it('skips unreadable files', async () => {
    const files = [
      diffFile('gone.rs', `${ROOT}/gone.rs`, [{ oldStart: 1, oldCount: 1, newStart: 1, newCount: 1 }]),
    ];
    await expect(buildDiffRequest(files, resolver, async () => {
      throw new Error('ENOENT');
    }, 2, ROOT)).rejects.toThrow(/No readable changed files/);
  });
});

describe('buildSelectionRequest', () => {
  const resolver = new TreeSitterResolver('/nonexistent-resources');

  it('expands a cursor line to the enclosing function', async () => {
    const sel: SelectionTarget = {
      filePath: 'src/vault.rs',
      absPath: `${ROOT}/src/vault.rs`,
      range: { startLine: 4, startCol: 5, endLine: 4, endCol: 30 },
      text: 'vault.balance += amount;',
      documentText: RUST_SOURCE,
    };
    const req = await buildSelectionRequest(sel, resolver);
    expect(req.mode).toBe('selection');
    expect(req.selection?.path).toBe('src/vault.rs');
    const lines = req.files[0]!.snippet.split('\n');
    expect(lines[0]).toContain('deposit');
  });
});

describe('refinePlanRanges', () => {
  const resolver = new TreeSitterResolver('/nonexistent-resources');

  it('snaps ranges and fills scopeRange', async () => {
    const plan = {
      steps: [
        {
          filePath: 'src/vault.rs',
          range: { startLine: 4, startCol: 1, endLine: 4, endCol: 999 },
        },
      ],
    };
    await refinePlanRanges(plan, resolver, ROOT, async () => RUST_SOURCE);
    const step = plan.steps[0]!;
    expect(step.range.endCol).toBeLessThan(999);
    expect(step.scopeRange).toBeDefined();
    expect(step.scopeRange!.startLine).toBe(3);
  });
});

describe('prompts', () => {
  it('system prompt pins the schema and JSON-only contract', () => {
    expect(SYSTEM_PROMPT).toContain('WalkthroughPlan');
    expect(SYSTEM_PROMPT).toContain('1-based');
    expect(SYSTEM_PROMPT).toContain('single JSON object');
  });

  it('orders the selection file first and includes line numbers', () => {
    const req = {
      mode: 'selection' as const,
      language: 'rust',
      files: [
        {
          path: 'src/other.rs',
          hunks: [{ start: 1, end: 30 }],
          snippet: '1│ other',
          astSummary: '',
        },
        {
          path: 'src/focus.rs',
          hunks: [{ start: 2, end: 3 }],
          snippet: '2│ focus',
          astSummary: '',
        },
      ],
      selection: { path: 'src/focus.rs', range: { startLine: 2, startCol: 1, endLine: 3, endCol: 2 } },
    };
    const prompt = buildUserPrompt(req, 10);
    expect(prompt.indexOf('src/focus.rs')).toBeLessThan(prompt.indexOf('src/other.rs'));
    expect(prompt).toContain('Maximum 10 steps');
  });

  it('caps ast summary entries', () => {
    const nodes = Array.from({ length: 100 }, (_, i) => ({
      category: 'call',
      text: `call${i}`,
      range: { startLine: i + 1, startCol: 1, endLine: i + 1, endCol: 5 },
    }));
    const summary = formatAstSummary(nodes);
    expect(summary).toContain('more symbols');
    expect(summary.split('\n').length).toBeLessThan(70);
  });
});
