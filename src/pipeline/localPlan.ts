import type { TreeSitterResolver, NodeCategory, SyntaxNode } from '../ast/TreeSitterResolver';
import type { DiffFile } from '../git/pure';
import { hunkLineRanges } from '../git/pure';
import type { WalkthroughPlan, WalkthroughStep, VariableMutation } from '../types/protocol';

const MAX_FILES = 12;
const MAX_NODES_PER_HUNK = 6;
const MAX_SOURCE_BYTES = 2_000_000;

const VALIDATE_HINT_RE = /require|assert|revert|ensure|check|verify|guard|panic|unwrap!/i;

export async function buildLocalPlan(
  files: DiffFile[],
  resolver: TreeSitterResolver,
  readText: (absPath: string) => Promise<string>,
  workspaceRoot: string,
  maxSteps: number,
): Promise<WalkthroughPlan> {
  const steps: WalkthroughStep[] = [];

  for (const f of files.slice(0, MAX_FILES)) {
    if (steps.length >= maxSteps) break;
    let source: string;
    try {
      source = await readText(f.absPath);
    } catch {
      continue;
    }
    source = source.slice(0, MAX_SOURCE_BYTES);
    const ast = await resolver.parse(f.absPath, source);
    const rel = relPath(workspaceRoot, f.absPath);

    for (const hunk of hunkLineRanges(f)) {
      const nodes = ast.nodes
        .filter((n) => intersects(n, hunk) && n.category !== 'function')
        .sort((a, b) => a.range.startLine - b.range.startLine)
        .slice(0, MAX_NODES_PER_HUNK);

      for (const node of nodes) {
        if (steps.length >= maxSteps) break;
        const scope = resolver.enclosingScope(ast, source, node.range.startLine);
        steps.push(stepFor(node, rel, scope?.range, scope?.name));
      }
    }
  }

  if (steps.length === 0) {
    throw new Error('Instant mode found no analyzable statements in the changed lines — try the AI walkthrough instead.');
  }

  return {
    summary: `Instant walkthrough — ${files.length} changed file${files.length === 1 ? '' : 's'} (local AST analysis, no model)`,
    entryPoint: steps[0]!.filePath,
    totalSteps: steps.length,
    steps,
  };
}

function stepFor(
  node: SyntaxNode,
  filePath: string,
  scopeRange: WalkthroughStep['scopeRange'],
  scopeName: string | undefined,
): WalkthroughStep {
  const variable = variableFor(node);
  const where = scopeName ? ` in \`${scopeName}\`` : '';
  const title = titleFor(node, variable);
  const base: WalkthroughStep = {
    stepIndex: 0,
    title,
    filePath,
    range: node.range,
    explanation: '',
  };
  if (scopeRange) base.scopeRange = scopeRange;
  if (variable) base.variable = variable;

  switch (node.category) {
    case 'declaration':
      base.explanation = `Local analysis: introduces \`${variable?.name ?? node.text}\`${where} — the initialization site for this value.`;
      break;
    case 'assignment':
      base.explanation = `Local analysis: mutates \`${variable?.name ?? 'value'}\`${where} — data changes hands here.`;
      break;
    case 'call':
      base.explanation = `Local analysis: call to \`${compact(node.text)}\`${where} — control or data flows out through this invocation.`;
      break;
    case 'security':
      base.explanation = `Local analysis: validation/invariant statement${where} — execution only proceeds past this line if the check holds.`;
      break;
    default:
      base.explanation = `Local analysis: statement${where} touched by this change.`;
  }
  if (node.category === 'security' && VALIDATE_HINT_RE.test(node.text)) {
    base.securityNote = 'Invariant check in the changed lines — instant mode flags the site; use the AI walkthrough for semantic analysis.';
  }
  return base;
}

function variableFor(node: SyntaxNode): VariableMutation | undefined {
  const name = extractName(node);
  if (!name) return undefined;
  let action: VariableMutation['action'];
  if (node.category === 'declaration') action = 'init';
  else if (node.category === 'assignment') action = 'mutate';
  else if (node.category === 'call') action = 'pass';
  else if (node.category === 'security') action = 'validate';
  else return undefined;
  return { name, action };
}

function extractName(node: SyntaxNode): string | undefined {
  const text = node.text.trim();
  if (node.category === 'assignment' || node.category === 'declaration') {
    const m = /^([A-Za-z_][\w.$]*)\s*(?::[^=]+)?=/.exec(text) ?? /^([A-Za-z_][\w.$]*)\s*(?:\+=|-=|\*=|\/=)/.exec(text);
    if (m?.[1]) return m[1];
  }
  if (node.category === 'call') {
    const m = /([A-Za-z_][\w.]*)\s*\(/.exec(text);
    if (m?.[1]) return m[1];
  }
  const first = /^([A-Za-z_]\w*)/.exec(text);
  return first?.[1];
}

function titleFor(node: SyntaxNode, variable: VariableMutation | undefined): string {
  const name = variable?.name ?? compact(node.text);
  switch (node.category) {
    case 'declaration':
      return `Init ${name}`;
    case 'assignment':
      return `Mutate ${name}`;
    case 'call':
      return `Call ${name}`;
    case 'security':
      return `Validate ${name}`;
    default:
      return `Inspect ${name}`;
  }
}

function compact(text: string): string {
  const t = text.replace(/\s+/g, ' ').trim();
  return t.length > 30 ? `${t.slice(0, 29)}…` : t;
}

function intersects(node: SyntaxNode, hunk: { start: number; end: number }): boolean {
  return node.range.startLine <= hunk.end && node.range.endLine >= hunk.start;
}

function relPath(root: string, absPath: string): string {
  const rel = absPath.startsWith(root) ? absPath.slice(root.length).replace(/^\/+/, '') : absPath;
  return rel.replace(/\\/g, '/');
}

export type { NodeCategory };
