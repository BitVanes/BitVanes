import { indentOf } from '../util/text';

const CALL_KEYWORDS = new Set([
  'if',
  'for',
  'while',
  'switch',
  'catch',
  'return',
  'fn',
  'func',
  'function',
  'match',
  'typeof',
  'new',
  'delete',
  'in',
  'of',
  'and',
  'or',
  'not',
  'with',
  'await',
  'yield',
  'defer',
  'go',
  'super',
  'emit',
]);

const FN_DECL_RE = /^\s*(?:(?:pub(?:\(.*?\))?\s+)?(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:fn|func|function|def)\s+([A-Za-z_]\w*))|(?:^\s*(?:public|private|internal|external|override|payable|view|pure|virtual)\s+)+(?:function\s+([A-Za-z_]\w*))|(?:^\s*function\s+([A-Za-z_]\w*))|(?:^\s*(?:pub\s+)?(?:struct|contract|library|interface|impl|trait|class|enum|modifier)\s+([A-Za-z_]\w*))/;

const ASSIGN_RE = /^(\s*)([A-Za-z_][\w.$]*)\s*(?::=|=[^=>]|\+=|-=|\*=|\/=|%=|\|=|&=|\^=|<<=|>>=|\?\?=|\|\|=|&&=)/;
const TYPED_DECL_RE = /^\s*(?:(?:let|var|const|mut)\s+)?([A-Za-z_][\w.$]*)\s*(?::\s*[^=]+)?=\s*(?!=)/;
const CALL_RE = /([A-Za-z_][\w.]*)\s*\(/g;
const CONTROL_RE = /^\s*(?:return\b|if\b|for\b|while\b|match\b|require\b|assert\b|revert\b|emit\b|raise\b|throw\b|check\b|ensure\b|verify\b|guard\b|panic!|unwrap!|expect\()/;

export interface FallbackNode {
  category: 'function' | 'declaration' | 'assignment' | 'call' | 'control';
  text: string;
  startLine: number;
  endLine: number;
  startCol: number;
  endCol: number;
}

function lineSpan(line: string): { startCol: number; endCol: number } {
  const trimmed = line.trim();
  if (trimmed === '') return { startCol: 1, endCol: 2 };
  return { startCol: line.indexOf(trimmed) + 1, endCol: line.trimEnd().length + 1 };
}

export function extractFallbackNodes(source: string): FallbackNode[] {
  const nodes: FallbackNode[] = [];
  const lines = source.split('\n');

  lines.forEach((line, i) => {
    const { startCol, endCol } = lineSpan(line);
    const ln = i + 1;

    const fn = FN_DECL_RE.exec(line);
    if (fn) {
      const name = fn[1] ?? fn[2] ?? fn[3] ?? fn[4];
      if (name) {
        nodes.push({ category: 'function', text: name, startLine: ln, endLine: ln, startCol, endCol });
      }
    }

    const assign = ASSIGN_RE.exec(line) ?? TYPED_DECL_RE.exec(line);
    if (assign) {
      const name = assign[2] ?? assign[1];
      if (name && !RESERVED.has(name)) {
        const isDecl = /^\s*(?:let|var|const|mut)\s+[A-Za-z_]/.test(line);
        nodes.push({
          category: isDecl ? 'declaration' : 'assignment',
          text: `${name} = …`,
          startLine: ln,
          endLine: ln,
          startCol,
          endCol,
        });
      }
    }

    if (CONTROL_RE.test(line)) {
      nodes.push({ category: 'control', text: line.trim().slice(0, 60), startLine: ln, endLine: ln, startCol, endCol });
    }

    CALL_RE.lastIndex = 0;
    let call: RegExpExecArray | null;
    while ((call = CALL_RE.exec(line)) !== null) {
      const name = call[1] ?? '';
      const root = name.split('.')[0] ?? '';
      if (!CALL_KEYWORDS.has(name) && !CALL_KEYWORDS.has(root) && !RESERVED.has(root)) {
        const at = call.index + 1;
        nodes.push({
          category: 'call',
          text: `${name}(…)`,
          startLine: ln,
          endLine: ln,
          startCol: at,
          endCol: at + name.length,
        });
      }
    }
  });

  return nodes;
}

const RESERVED = new Set([
  'import',
  'package',
  'module',
  'use',
  'from',
  'pragma',
  'const',
  'let',
  'var',
  'type',
  'interface',
  'struct',
  'enum',
  'export',
  'default',
  'await',
  'async',
  'yield',
  'return',
  'null',
  'true',
  'false',
  'this',
  'self',
  'Self',
  'super',
  'contract',
  'library',
  'bool',
  'address',
  'string',
  'uint',
  'int',
]);

export function fallbackFunctionRanges(source: string): Array<{ name: string; startLine: number; endLine: number }> {
  const lines = source.split('\n');
  const out: Array<{ name: string; startLine: number; endLine: number }> = [];
  const decls: Array<{ name: string; line: number }> = [];
  lines.forEach((line, i) => {
    const fn = FN_DECL_RE.exec(line);
    if (fn) {
      decls.push({ name: fn[1] ?? fn[2] ?? fn[3] ?? fn[4] ?? '(anonymous)', line: i });
    }
  });
  for (const d of decls) {
    const indent = indentOf(lines[d.line] ?? '');
    let end = d.line;
    for (let i = d.line + 1; i < lines.length; i++) {
      const l = lines[i] ?? '';
      if (l.trim() === '') continue;
      if (indentOf(l) <= indent) break;
      end = i;
    }
    out.push({ name: d.name, startLine: d.line + 1, endLine: end + 1 });
  }
  return out;
}
