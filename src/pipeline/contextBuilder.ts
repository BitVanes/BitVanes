import type { ContextFile, WalkthroughRequest } from '../llm/prompts';
import { formatAstSummary } from '../llm/prompts';
import type { TreeSitterResolver } from '../ast/TreeSitterResolver';
import type { DiffFile, SelectionTarget } from '../git/GitProvider';
import { hunkLineRanges } from '../git/pure';
import { numberLines, toRelativePath } from '../util/text';

export type ReadText = (absPath: string) => Promise<string>;
export type GetDiff = () => Promise<DiffFile[]>;

export const MAX_FILES = 12;
const MAX_SOURCE_BYTES = 2_000_000;

export function mergeWindows(
  hunks: Array<{ start: number; end: number }>,
  contextLines: number,
  lineCount: number,
): Array<{ start: number; end: number }> {
  const sorted = [...hunks].sort((a, b) => a.start - b.start);
  const windows: Array<{ start: number; end: number }> = [];
  for (const h of sorted) {
    const w = {
      start: Math.max(1, h.start - contextLines),
      end: Math.min(lineCount, h.end + contextLines),
    };
    const last = windows[windows.length - 1];
    if (last && w.start <= last.end + 1) {
      last.end = Math.max(last.end, w.end);
    } else {
      windows.push(w);
    }
  }
  return windows;
}

export async function buildDiffRequest(
  files: DiffFile[],
  resolver: TreeSitterResolver,
  readText: ReadText,
  contextLines: number,
  workspaceRoot: string,
): Promise<WalkthroughRequest> {
  const contextFiles: ContextFile[] = [];
  let language: string | null = null;

  // Largest changes first so the file cap drops the least important files.
  const ordered = [...files].sort((a, b) => totalChangedLines(b) - totalChangedLines(a));
  for (const f of ordered.slice(0, MAX_FILES)) {
    let source: string;
    try {
      source = await readText(f.absPath);
    } catch {
      continue;
    }
    source = source.slice(0, MAX_SOURCE_BYTES);
    if (!language) language = resolver.detectLanguage(f.absPath);

    const lines = source.split('\n');
    const hunks = hunkLineRanges(f);
    const windows = mergeWindows(hunks, contextLines, lines.length);
    const snippet = windows.map((w) => numberLines(lines, w.start, w.end)).join('\n⋯\n');

    const ast = await resolver.parse(f.absPath, source);
    const relevant = ast.nodes.filter((n) => hunks.some((h) => n.range.startLine >= h.start - 3 && n.range.startLine <= h.end + 3));

    contextFiles.push({
      path: toRelativePath(workspaceRoot, f.absPath),
      status: f.status,
      hunks,
      snippet,
      astSummary: formatAstSummary(relevant),
    });
  }

  if (contextFiles.length === 0) {
    throw new Error('No readable changed files found for the walkthrough.');
  }
  return { mode: 'diff', language, files: contextFiles, coverage: { included: contextFiles.length, total: files.length } };
}

export async function buildSelectionRequest(
  sel: SelectionTarget,
  resolver: TreeSitterResolver,
): Promise<WalkthroughRequest> {
  const source = sel.documentText.slice(0, MAX_SOURCE_BYTES);
  const ast = await resolver.parse(sel.absPath, source);
  const lines = source.split('\n');

  const enclosing = resolver.enclosingScope(ast, source, sel.range.startLine);
  const startLine = Math.max(1, Math.min(enclosing?.range.startLine ?? sel.range.startLine, sel.range.startLine));
  const endLine = Math.min(lines.length, Math.max(enclosing?.range.endLine ?? sel.range.endLine, sel.range.endLine));
  const snippet = numberLines(lines, startLine, endLine);

  const inScope = ast.nodes.filter((n) => n.range.startLine >= startLine && n.range.endLine <= endLine + 1);

  return {
    mode: 'selection',
    language: ast.language ?? resolver.detectLanguage(sel.absPath),
    files: [
      {
        path: sel.filePath,
        hunks: [{ start: sel.range.startLine, end: sel.range.endLine }],
        snippet,
        astSummary: formatAstSummary(inScope),
      },
    ],
    selection: { path: sel.filePath, range: sel.range },
  };
}

export async function refinePlanRanges(
  plan: { steps: Array<{ filePath: string; range: { startLine: number; startCol: number; endLine: number; endCol: number }; scopeRange?: { startLine: number; startCol: number; endLine: number; endCol: number } }> },
  resolver: TreeSitterResolver,
  workspaceRoot: string,
  readText: ReadText,
): Promise<void> {
  const astCache = new Map<string, { source: string; ast: Awaited<ReturnType<TreeSitterResolver['parse']>> }>();

  for (const step of plan.steps) {
    const abs = resolveWithin(workspaceRoot, step.filePath);
    if (!abs) continue;
    let entry = astCache.get(abs);
    if (!entry) {
      try {
        const source = (await readText(abs)).slice(0, MAX_SOURCE_BYTES);
        entry = { source, ast: await resolver.parse(abs, source) };
        astCache.set(abs, entry);
      } catch {
        continue;
      }
    }
    step.range = resolver.snapRange(entry.ast, entry.source, step.range);
    if (!step.scopeRange) {
      const scope = resolver.enclosingScope(entry.ast, entry.source, step.range.startLine);
      if (scope) step.scopeRange = scope.range;
    }
  }
}

function resolveWithin(root: string, filePath: string): string | undefined {
  const norm = filePath.replace(/\\/g, '/').replace(/^\.\//, '');
  const r = root.replace(/[\\/]+$/, '');
  if (norm.startsWith('/') || /^[A-Za-z]:/.test(norm)) {
    return norm === r || norm.startsWith(`${r}/`) ? norm : undefined;
  }
  return `${r}/${norm}`;
}

function totalChangedLines(f: DiffFile): number {
  return hunkLineRanges(f).reduce((acc, h) => acc + (h.end - h.start + 1), 0);
}
