import { describe, expect, it } from 'vitest';
import { planRangeToPositions, toRelativePath, clampRangeToDocument, END_OF_LINE_COL } from '../src/util/text';

const DOC = ['let a = 1;', 'let bb = 22;', '', 'fn long_line_here() {', '}'];

function docLine(i: number): string {
  return DOC[i] ?? '';
}

describe('planRangeToPositions', () => {
  it('converts 1-based inclusive ranges to 0-based positions', () => {
    expect(planRangeToPositions({ startLine: 1, startCol: 5, endLine: 1, endCol: 10 }, DOC.length, docLine)).toEqual({
      startLine: 0,
      startCharacter: 4,
      endLine: 0,
      endCharacter: 9,
    });
  });

  it('extends a multi-line range with endCol 1 to the end of the last line', () => {
    const p = planRangeToPositions({ startLine: 1, startCol: 1, endLine: 2, endCol: 1 }, DOC.length, docLine);
    expect(p).toEqual({ startLine: 0, startCharacter: 0, endLine: 1, endCharacter: 12 });
  });

  it('extends a multi-line range with the end-of-line sentinel', () => {
    const p = planRangeToPositions({ startLine: 4, startCol: 1, endLine: 5, endCol: END_OF_LINE_COL }, DOC.length, docLine);
    expect(p).toEqual({ startLine: 3, startCharacter: 0, endLine: 4, endCharacter: 1 });
  });

  it('gives a single-line zero-width range a minimum 1-character width', () => {
    const p = planRangeToPositions({ startLine: 2, startCol: 5, endLine: 2, endCol: 1 }, DOC.length, docLine);
    // line 2 (0-based 1) is "let bb = 22;" — must not be zero-width
    expect(p.startLine).toBe(1);
    expect(p.endLine).toBe(1);
    expect(p.endCharacter).toBeGreaterThan(p.startCharacter);
  });

  it('clamps ranges past EOF onto the last line', () => {
    const p = planRangeToPositions({ startLine: 99, startCol: 1, endLine: 100, endCol: 4 }, DOC.length, docLine);
    expect(p.startLine).toBe(DOC.length - 1);
    expect(p.endLine).toBe(DOC.length - 1);
    expect(p.endCharacter).toBeLessThanOrEqual(docLine(DOC.length - 1).length);
  });

  it('clamps columns to the line length', () => {
    const p = planRangeToPositions({ startLine: 1, startCol: 1, endLine: 1, endCol: 9_999 }, DOC.length, docLine);
    expect(p.endCharacter).toBe(DOC[0]!.length);
  });
});

describe('toRelativePath', () => {
  it('strips the root with a separator boundary', () => {
    expect(toRelativePath('/ws/app', '/ws/app/src/x.rs')).toBe('src/x.rs');
  });

  it('does not treat a string-prefix sibling as the root', () => {
    expect(toRelativePath('/ws/app', '/ws/application/x.rs')).toBe('/ws/application/x.rs');
  });

  it('returns empty for the root itself', () => {
    expect(toRelativePath('/ws/app', '/ws/app')).toBe('');
  });
});

describe('clampRangeToDocument', () => {
  it('clamps end columns to the line content length + 1', () => {
    const r = clampRangeToDocument({ startLine: 2, startCol: 1, endLine: 2, endCol: END_OF_LINE_COL }, DOC);
    expect(r.endLine).toBe(2);
    expect(r.endCol).toBe(13); // "let bb = 22;" is 12 chars
  });

  it('keeps a minimum endCol of 1 on empty lines', () => {
    const r = clampRangeToDocument({ startLine: 3, startCol: 1, endLine: 3, endCol: END_OF_LINE_COL }, DOC);
    expect(r.endCol).toBe(1);
  });

  it('clamps lines beyond EOF', () => {
    const r = clampRangeToDocument({ startLine: 1, startCol: 1, endLine: 999, endCol: 1 }, DOC);
    expect(r.endLine).toBe(DOC.length);
  });
});
