import * as path from 'path';
import type { Range } from '../types/protocol';

/** Sentinel endCol meaning "through the end of the line" (must match protocol MAX_COLS). */
export const END_OF_LINE_COL = 100_000;

export function indentOf(line: string): number {
  const m = /^[\t ]*/.exec(line);
  return m ? m[0].length : 0;
}

function looksLikeCloser(line: string): boolean {
  return /^\s*[})\]]/.test(line);
}

export function expandToBlock(lines: readonly string[], anchorLine0: number): { startLine: number; endLine: number } {
  const n = lines.length;
  let anchor = anchorLine0;
  while (anchor < n && (lines[anchor] ?? '').trim() === '') anchor++;
  if (anchor >= n) {
    return { startLine: Math.max(0, anchorLine0 - 1), endLine: Math.max(0, anchorLine0) };
  }
  const anchorIndent = indentOf(lines[anchor] ?? '');

  let start = anchor;
  for (let i = anchor - 1; i >= 0; i--) {
    const l = lines[i] ?? '';
    if (l.trim() === '') continue;
    if (indentOf(l) >= anchorIndent) {
      start = i;
      continue;
    }
    if (!looksLikeCloser(l)) {
      start = i;
    }
    break;
  }

  let end = anchor;
  for (let i = anchor + 1; i < n; i++) {
    const l = lines[i] ?? '';
    if (l.trim() === '') continue;
    if (indentOf(l) < anchorIndent) break;
    end = i;
  }

  return { startLine: start, endLine: end };
}

export function clampRangeToDocument(range: Range, lines: readonly string[]): Range {
  const lineCount = Math.max(1, lines.length);
  const startLine = Math.min(Math.max(1, range.startLine), lineCount);
  const endLine = Math.min(Math.max(startLine, range.endLine), lineCount);
  const startLineText = lines[startLine - 1] ?? '';
  const endLineText = lines[endLine - 1] ?? '';
  const maxStartCol = startLineText.trimEnd().length + 1;
  const maxEndCol = Math.max(1, endLineText.trimEnd().length + 1);
  return {
    startLine,
    startCol: Math.min(Math.max(1, range.startCol), Math.max(1, maxStartCol)),
    endLine,
    endCol: Math.min(Math.max(1, range.endCol), Math.max(1, maxEndCol)),
  };
}

export function rangeContainsLine(range: Range, line: number): boolean {
  return line >= range.startLine && line <= range.endLine;
}

export function spanLines(range: Range): number {
  return range.endLine - range.startLine;
}

export function toDisplayPath(p: string): string {
  const parts = p.split('/');
  return parts.length > 2 ? parts.slice(-2).join('/') : p;
}

/**
 * Convert a 1-based protocol Range into clamped 0-based (line, character) pairs.
 * Multi-line ranges whose endCol is 1 or the end-of-line sentinel extend to the
 * end of the last line so the final line is never dropped; single-line ranges
 * get a minimum one-character width so the spotlight stays visible.
 */
export function planRangeToPositions(
  r: Range,
  lineCount: number,
  lineText: (line0: number) => string,
): { startLine: number; startCharacter: number; endLine: number; endCharacter: number } {
  const maxLine = Math.max(0, lineCount - 1);
  const startLine = Math.min(Math.max(0, r.startLine - 1), maxLine);
  const endLine = Math.min(Math.max(startLine, r.endLine - 1), maxLine);
  const startLen = lineText(startLine).length;
  const startCharacter = Math.min(Math.max(0, r.startCol - 1), startLen);
  const endLen = lineText(endLine).length;
  let endCharacter: number;
  if (endLine > startLine && (r.endCol <= 1 || r.endCol >= END_OF_LINE_COL)) {
    endCharacter = endLen;
  } else {
    endCharacter = Math.min(Math.max(0, r.endCol - 1), endLen);
    if (endLine === startLine && endCharacter <= startCharacter && endLen > startCharacter) {
      endCharacter = Math.min(endLen, startCharacter + 1);
    }
  }
  return { startLine, startCharacter, endLine, endCharacter };
}

/** Repo/workspace-relative path for prompts and step filePaths (separator-safe). */
export function toRelativePath(root: string, absPath: string): string {
  const normRoot = root.replace(/[\\/]+$/, '');
  if (absPath === normRoot) return '';
  const rel = absPath.startsWith(`${normRoot}${path.sep}`) ? absPath.slice(normRoot.length + 1) : absPath;
  return rel.replace(/\\/g, '/');
}

export function numberLines(lines: readonly string[], from: number, to: number): string {
  const out: string[] = [];
  const width = String(to).length;
  for (let n = from; n <= to && n - 1 < lines.length; n++) {
    out.push(`${String(n).padStart(width)}│ ${lines[n - 1] ?? ''}`);
  }
  return out.join('\n');
}
