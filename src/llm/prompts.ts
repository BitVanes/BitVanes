import type { Range } from '../types/protocol';

export interface ContextFile {
  path: string;
  status?: string;
  hunks: Array<{ start: number; end: number }>;
  snippet: string;
  astSummary: string;
}

export interface WalkthroughRequest {
  mode: 'diff' | 'selection';
  language: string | null;
  files: ContextFile[];
  selection?: { path: string; range: Range; note?: string };
  /** How many changed files the prompt actually covers (truncation honesty). */
  coverage?: { included: number; total: number };
}

const MAX_FILE_CHARS = 12_000;
const MAX_TOTAL_CHARS = 56_000;
const MAX_AST_ENTRIES = 60;

export const SYSTEM_PROMPT = `You are BitVanes, a senior code reviewer that converts source diffs and code paths into interactive, step-by-step walkthroughs inside a code editor.

OUTPUT CONTRACT — read carefully:
- Respond with a single JSON object and nothing else. No prose, no markdown fences, no comments.
- The JSON must match this TypeScript schema exactly:

interface Range { startLine: number; startCol: number; endLine: number; endCol: number }
interface VariableMutation { name: string; action: 'init' | 'pass' | 'validate' | 'mutate' | 'commit' | 'return'; stateBefore?: string; stateAfter?: string }
interface WalkthroughStep { stepIndex: number; title: string; filePath: string; range: Range; scopeRange?: Range; variable?: VariableMutation; explanation: string; securityNote?: string }
interface WalkthroughPlan { summary: string; entryPoint: string; totalSteps: number; steps: WalkthroughStep[] }

COORDINATES:
- All lines and columns are 1-based. range covers the exact statement or token span in the CURRENT file content provided to you (the new side of the diff).
- scopeRange covers the enclosing function, method, or block that the step executes within.
- endLine is inclusive. endCol is exclusive: the 1-based column just past the last character of the span. To cover through the end of a line, use endCol = line length + 1.
- filePath must be exactly one of the file paths given in the prompt.

TRACING RULES — order steps by execution and data flow, not merely top-to-bottom:
1. Input reception (parameters, msg, events, reads).
2. Invariant validation (guards, checks, auth, require/assert, boundary conditions).
3. State transitions (computations, mutations of variables and storage).
4. Persistence and return (writes, commits, emits, returns).
For every important variable, set "variable": use action "init" for creation, "pass" when a value flows into a call or another scope, "validate" when checked, "mutate" when changed, "commit" when persisted/emitted/stored, "return" when returned. stateBefore/stateAfter are short symbolic values like "0", "None", "tx.input", "msg.value - fee". Keep them under 40 characters.

SECURITY NOTES — set "securityNote" (1-2 factual sentences) when a step involves: access control or authorization checks, unchecked external calls or oracles, integer overflow/underflow or rounding, reentrancy or state-read-after-write risk, secrets or key material, panics/unwraps that can abort, injection surfaces, or resource/limit enforcement. Do not invent issues; if the step is clean, omit securityNote.

STYLE: summary is 1-2 sentences describing what the change/path does. entryPoint names the function or file where the flow begins. Titles are imperative, 3-8 words, e.g. "Validate signature before debit". Explanations are 1-3 sentences written for a senior engineer reading code for the first time.`;

export type WalkthroughStyle = 'expert' | 'standard' | 'learner';

const STYLE_DIRECTIVES: Record<WalkthroughStyle, string> = {
  expert: `## Audience: Expert
The reader is a senior engineer fluent in this language and its ecosystem.
- Titles: 2-5 words. Explanations: one dense, high-signal sentence.
- Never explain language syntax, standard library calls, or common idioms.
- Focus on intent, invariants, ordering/concurrency, edge cases, and risk.
- securityNote must be precise and actionable — name the exact condition under which it bites.`,
  standard: `## Audience: Standard
The reader is a working developer competent in this language but new to this code.
- Titles: 3-8 words. Explanations: 1-3 sentences.
- Explain what the code does and why it exists in the flow. Skip syntax lessons.`,
  learner: `## Audience: Learner
The reader may be new to programming, or reviewing AI-generated code they did not write and do not fully understand.
- Explanations: 2-4 sentences in plain language.
- The first time a non-obvious construct appears (operator, decorator, pattern match, language-specific idiom), explain it in a short clause.
- Define jargon on first use.
- Explain why the pattern is used, not just what happens.
- Neutral, respectful tone — never condescending.`,
};

export function buildSystemPrompt(style: WalkthroughStyle = 'standard'): string {
  return `${SYSTEM_PROMPT}\n\n${STYLE_DIRECTIVES[style] ?? STYLE_DIRECTIVES.standard}`;
}

export function buildUserPrompt(req: WalkthroughRequest, maxSteps: number): string {
  const parts: string[] = [];
  parts.push(`## Task`);
  parts.push(
    req.mode === 'diff'
      ? 'Produce a WalkthroughPlan for the pending code changes below. Walk the changed code path in execution order so a reviewer can follow exactly what the change does.'
      : 'Produce a WalkthroughPlan for the selected code path below. Trace how data flows through it: from entry, through validation, into state changes, and out via persistence or return.',
  );
  parts.push(`Maximum ${maxSteps} steps. Return ONLY the JSON object.`);
  parts.push('');
  parts.push(`## Language`);
  parts.push(req.language ?? 'unknown (inferred)');
  parts.push('');

  parts.push('## Files');
  const ordered = orderFiles(req);
  let total = 0;
  const included: string[] = [];
  const skipped: string[] = [];

  for (const f of ordered) {
    const header =
      `### ${f.path}${f.status ? ` (${f.status})` : ''}\n` +
      (f.hunks.length > 0 ? `Changed lines: ${f.hunks.map((h) => (h.start === h.end ? `${h.start}` : `${h.start}-${h.end}`)).join(', ')}\n` : '') +
      '\n```\n' +
      f.snippet.slice(0, MAX_FILE_CHARS) +
      '\n```\n' +
      (f.astSummary ? `\nAST symbols:\n${f.astSummary}\n` : '');
    if (total + header.length > MAX_TOTAL_CHARS) {
      skipped.push(f.path);
      continue;
    }
    included.push(header);
    total += header.length;
  }

  for (const h of included) parts.push(h);
  if (skipped.length > 0) {
    parts.push(`(Omitted for prompt budget: ${skipped.join(', ')})`);
  }

  if (req.selection) {
    parts.push('');
    parts.push('## Focus');
    parts.push(
      `The reviewer selected ${req.selection.path}:${req.selection.range.startLine}-${req.selection.range.endLine}. ` +
        'Center the walkthrough on this code path and the functions it directly calls within the provided context.' +
        (req.selection.note ? `\n${req.selection.note}` : ''),
    );
  }

  parts.push('');
  parts.push('## Requirements');
  parts.push('- Steps must only reference files and line numbers present in the context above.');
  parts.push('- Prefer fewer, meaningful steps over trivial ones; merge consecutive trivial lines.');
  parts.push('- Every step needs a concrete range; do not point at files as a whole.');
  return parts.join('\n');
}

export function buildRepairPrompt(rawOutput: string, error: string): string {
  return [
    'Your previous response was not a valid WalkthroughPlan JSON. Return the corrected, complete JSON object only — no prose, no fences.',
    '',
    'Validation errors:',
    error,
    '',
    'Previous response (for reference):',
    rawOutput.slice(0, 4_000),
  ].join('\n');
}

export function formatAstSummary(
  nodes: Array<{ category: string; text: string; range: Range }>,
  limit = MAX_AST_ENTRIES,
): string {
  const shown = nodes.slice(0, limit);
  const lines = shown.map((n) => `- L${n.range.startLine}${n.range.startLine !== n.range.endLine ? `-L${n.range.endLine}` : ''} ${n.category}: ${n.text.replace(/\n/g, ' ').slice(0, 90)}`);
  if (nodes.length > shown.length) lines.push(`(…${nodes.length - shown.length} more symbols)`);
  return lines.join('\n');
}

function orderFiles(req: WalkthroughRequest): ContextFile[] {
  const focus = req.selection?.path;
  return [...req.files].sort((a, b) => {
    if (focus) {
      if (a.path === focus && b.path !== focus) return -1;
      if (b.path === focus && a.path !== focus) return 1;
    }
    return totalChangedLines(b) - totalChangedLines(a);
  });
}

function totalChangedLines(f: ContextFile): number {
  return f.hunks.reduce((acc, h) => acc + (h.end - h.start + 1), 0);
}
