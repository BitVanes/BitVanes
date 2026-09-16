import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { Parser, Language, type Tree, type Node as TSNode } from 'web-tree-sitter';
import type { Range } from '../types/protocol';
import { clampRangeToDocument, expandToBlock, indentOf } from '../util/text';
import { extractFallbackNodes, fallbackFunctionRanges, type FallbackNode } from './LineFallback';

export type NodeCategory = 'function' | 'declaration' | 'assignment' | 'call' | 'control' | 'security';

export interface SyntaxNode {
  category: NodeCategory;
  type: string;
  text: string;
  range: Range;
}

export interface EnclosingScope {
  name: string;
  range: Range;
}

export interface FileAst {
  absPath: string;
  language: string | null;
  tree: Tree | null;
  nodes: SyntaxNode[];
  namedRanges: Array<{ range: Range; type: string; text: string }> | null;
  fallback: boolean;
}

interface LangProfile {
  grammar: string;
  functions: Set<string>;
  declarations: Set<string>;
  assignments: Set<string>;
  calls: Set<string>;
  params: Set<string>;
  security: Set<string>;
}

const S: (xs: string[]) => Set<string> = (xs) => new Set(xs);

const PROFILES: Record<string, LangProfile> = {
  rust: {
    grammar: 'rust',
    functions: S(['function_item', 'function_signature_item']),
    declarations: S(['let_declaration', 'const_item', 'static_item', 'struct_item', 'enum_item', 'impl_item', 'trait_item', 'field_declaration_item']),
    assignments: S(['assignment_expression', 'compound_assignment_expression']),
    calls: S(['call_expression', 'macro_invocation', 'method_call_expression']),
    params: S(['parameters', 'parameter', 'closure_parameters']),
    security: S(['try_expression', 'panic_expression']),
  },
  go: {
    grammar: 'go',
    functions: S(['function_declaration', 'method_declaration']),
    declarations: S(['short_var_declaration', 'var_declaration', 'const_declaration', 'type_declaration']),
    assignments: S(['assignment_statement', 'inc_statement', 'dec_statement']),
    calls: S(['call_expression']),
    params: S(['parameter_list', 'parameter_declaration']),
    security: S([]),
  },
  solidity: {
    grammar: 'solidity',
    functions: S(['function_definition', 'modifier_definition', 'constructor_definition', 'fallback_definition', 'receive_definition', 'fallback_receive_definition']),
    declarations: S(['state_variable_declaration', 'variable_declaration_statement', 'variable_declaration', 'variable_declaration_tuple', 'contract_declaration', 'struct_declaration', 'event_definition']),
    assignments: S(['assignment_expression', 'augmented_assignment_expression']),
    calls: S(['call_expression', 'emit_statement', 'revert_statement', 'try_statement']),
    params: S(['parameter_list', 'parameter']),
    security: S(['emit_statement', 'revert_statement', 'try_statement', 'unchecked_block']),
  },
  typescript: {
    grammar: 'typescript',
    functions: S(['function_declaration', 'method_definition', 'function_signature']),
    declarations: S(['variable_declarator', 'lexical_declaration', 'class_declaration', 'interface_declaration', 'import_statement', 'property_signature']),
    assignments: S(['assignment_expression', 'augmented_assignment_expression']),
    calls: S(['call_expression', 'new_expression']),
    params: S(['formal_parameters', 'required_parameter', 'optional_parameter']),
    security: S(['try_statement', 'throw_statement']),
  },
  tsx: {
    grammar: 'tsx',
    functions: S(['function_declaration', 'method_definition', 'function_signature']),
    declarations: S(['variable_declarator', 'lexical_declaration', 'class_declaration', 'interface_declaration', 'import_statement', 'property_signature']),
    assignments: S(['assignment_expression', 'augmented_assignment_expression']),
    calls: S(['call_expression', 'new_expression']),
    params: S(['formal_parameters', 'required_parameter', 'optional_parameter']),
    security: S(['try_statement', 'throw_statement']),
  },
  python: {
    grammar: 'python',
    functions: S(['function_definition', 'decorated_definition']),
    declarations: S(['assignment', 'augmented_assignment', 'class_definition', 'import_statement', 'import_from_statement']),
    assignments: S(['assignment', 'augmented_assignment']),
    calls: S(['call']),
    params: S(['parameters', 'identifier']),
    security: S(['raise_statement', 'try_statement', 'with_statement', 'assert_statement']),
  },
};

const LANG_BY_EXT: Record<string, string> = {
  rs: 'rust',
  go: 'go',
  sol: 'solidity',
  ts: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  tsx: 'tsx',
  py: 'python',
  pyi: 'python',
};

const NON_SNAPPABLE = new Set(['source_file', 'program', 'translation_unit', 'chunk', 'module', 'comment', 'block_comment', 'line_comment', 'expression_statement']);

export class TreeSitterResolver {
  private parser: Parser | null = null;
  private parserInit: Promise<Parser> | null = null;
  private readonly langs = new Map<string, Promise<Language>>();
  private readonly grammarDir: string;
  private readonly runtimeWasm: string;
  private wasmUsable: boolean | null = null;

  constructor(private resourcesDir: string) {
    this.grammarDir = join(resourcesDir, 'grammars');
    this.runtimeWasm = join(resourcesDir, 'tree-sitter.wasm');
  }

  private runtimeAvailable(): boolean {
    if (this.wasmUsable === null) {
      this.wasmUsable = existsSync(this.runtimeWasm);
    }
    return this.wasmUsable;
  }

  detectLanguage(absPath: string): string | null {
    const ext = absPath.split('.').pop()?.toLowerCase() ?? '';
    return LANG_BY_EXT[ext] ?? null;
  }

  supported(absPath: string): boolean {
    return this.detectLanguage(absPath) !== null;
  }

  private async ensureParser(): Promise<Parser> {
    if (this.parser) return this.parser;
    if (!this.parserInit) {
      this.parserInit = (async () => {
        const initArgs: Record<string, unknown> = {
          locateFile: () => this.runtimeWasm,
        };
        try {
          await Parser.init(initArgs);
        } catch {
          await Parser.init();
        }
        const p = new Parser();
        this.parser = p;
        return p;
      })();
      this.parserInit.catch(() => {
        this.wasmUsable = false;
        this.parserInit = null;
      });
    }
    return this.parserInit;
  }

  private async loadLanguage(name: string): Promise<Language | null> {
    const profile = PROFILES[name];
    if (!profile || !this.runtimeAvailable()) return null;
    const wasmPath = join(this.grammarDir, `tree-sitter-${profile.grammar}.wasm`);
    if (!existsSync(wasmPath)) return null;
    if (!this.langs.has(name)) {
      const load = (async () => {
        await this.ensureParser();
        return Language.load(wasmPath);
      })();
      load.catch(() => this.langs.delete(name));
      this.langs.set(name, load);
    }
    try {
      return await this.langs.get(name)!;
    } catch {
      return null;
    }
  }

  async parse(absPath: string, source: string): Promise<FileAst> {
    const lang = this.detectLanguage(absPath);
    if (!lang) return this.fallbackAst(absPath, source);

    try {
      const language = await this.loadLanguage(lang);
      if (!language) return this.fallbackAst(absPath, source);
      const parser = await this.ensureParser();
      parser.setLanguage(language);
      const tree = parser.parse(source);
      const profile = PROFILES[lang];
      if (!tree || !tree.rootNode || !profile) return this.fallbackAst(absPath, source);

      const nodes: SyntaxNode[] = [];
      const namedRanges: Array<{ range: Range; type: string; text: string }> = [];
      this.walk(tree.rootNode, profile, nodes, namedRanges);
      return { absPath, language: lang, tree, nodes, namedRanges, fallback: false };
    } catch {
      return this.fallbackAst(absPath, source);
    }
  }

  private fallbackAst(absPath: string, source: string): FileAst {
    return {
      absPath,
      language: null,
      tree: null,
      nodes: fallbackToSyntaxNodes(extractFallbackNodes(source)),
      namedRanges: null,
      fallback: true,
    };
  }

  private walk(
    node: TSNode,
    profile: LangProfile,
    out: SyntaxNode[],
    namedRanges: Array<{ range: Range; type: string; text: string }>,
  ): void {
    namedRanges.push({ range: tsRange(node), type: node.type, text: safeText(node) });
    let category: NodeCategory | undefined;
    if (profile.security.has(node.type)) category = 'security';
    else if (profile.functions.has(node.type)) category = 'function';
    else if (profile.declarations.has(node.type)) category = 'declaration';
    else if (profile.assignments.has(node.type)) category = 'assignment';
    else if (profile.calls.has(node.type)) category = 'call';
    if (category) {
      out.push({ category, type: node.type, text: safeText(node).slice(0, 80), range: tsRange(node) });
    }
    for (const child of node.namedChildren) {
      if (child) this.walk(child, profile, out, namedRanges);
    }
  }

  enclosingScope(ast: FileAst, source: string, line: number): EnclosingScope | undefined {
    if (ast.tree && ast.namedRanges) {
      let best: { range: Range; type: string; text: string } | undefined;
      for (const n of ast.nodes) {
        if (n.category === 'function' && n.range.startLine <= line && line <= n.range.endLine) {
          if (!best || span(n.range) < span(best.range)) best = n;
        }
      }
      if (best) {
        return { name: (best.text.split('\n')[0] ?? '').trim().slice(0, 80) || best.type, range: best.range };
      }
    }
    for (const f of fallbackFunctionRanges(source)) {
      if (f.startLine <= line && line <= f.endLine) {
        return { name: f.name, range: { startLine: f.startLine, startCol: 1, endLine: f.endLine, endCol: Number.MAX_SAFE_INTEGER } };
      }
    }
    return undefined;
  }

  snapRange(ast: FileAst, source: string, target: Range): Range {
    const lines = source.split('\n');
    const clamped = clampRangeToDocument(target, lines);

    if (ast.namedRanges && ast.namedRanges.length > 0) {
      const targetSpan = span(clamped);
      const maxSpan = targetSpan + 4;

      let bestContained: { range: Range; type: string } | undefined;
      for (const n of ast.namedRanges) {
        if (NON_SNAPPABLE.has(n.type)) continue;
        if (!containsRange(n.range, clamped)) continue;
        if (span(n.range) > maxSpan) continue;
        if (!bestContained || span(n.range) < span(bestContained.range)) bestContained = n;
      }
      if (bestContained) return clampRangeToDocument(bestContained.range, lines);

      const center = {
        line: Math.floor((clamped.startLine + clamped.endLine) / 2),
        col: Math.floor((clamped.startCol + clamped.endCol) / 2),
      };
      let centerNode: { range: Range; type: string } | undefined;
      for (const n of ast.namedRanges) {
        if (NON_SNAPPABLE.has(n.type)) continue;
        const r = n.range;
        if (
          (r.startLine < center.line || (r.startLine === center.line && r.startCol <= center.col)) &&
          (r.endLine > center.line || (r.endLine === center.line && r.endCol >= center.col)) &&
          span(r) <= maxSpan
        ) {
          if (!centerNode || span(r) < span(centerNode.range)) centerNode = n;
        }
      }
      if (centerNode) return clampRangeToDocument(centerNode.range, lines);
    }

    const startLine = clamped.startLine - 1;
    const endLine = clamped.endLine - 1;
    const first = (lines[startLine] ?? '').trimStart();
    const last = (lines[endLine] ?? '').trimEnd();
    if (first === '' && last === '') return clamped;
    return {
      startLine: clamped.startLine,
      startCol: clamped.startLine === clamped.endLine && first === '' ? 1 : (lines[startLine] ?? '').length - (lines[startLine] ?? '').trimStart().length + 1,
      endLine: clamped.endLine,
      endCol: last.length + 1,
    };
  }
}

function fallbackToSyntaxNodes(nodes: FallbackNode[]): SyntaxNode[] {
  return nodes.map((n) => ({
    category: n.category,
    type: `fallback_${n.category}`,
    text: n.text,
    range: { startLine: n.startLine, startCol: n.startCol, endLine: n.endLine, endCol: n.endCol },
  }));
}

function tsRange(node: TSNode): Range {
  return {
    startLine: node.startPosition.row + 1,
    startCol: node.startPosition.column + 1,
    endLine: node.endPosition.row + 1,
    endCol: node.endPosition.column + 1,
  };
}

function safeText(node: TSNode): string {
  try {
    return node.text;
  } catch {
    return '';
  }
}

function span(r: Range): number {
  return r.endLine - r.startLine;
}

function containsRange(outer: Range, inner: Range): boolean {
  if (outer.startLine > inner.startLine) return false;
  if (outer.endLine < inner.endLine) return false;
  if (outer.startLine === inner.startLine && outer.startCol > inner.startCol) return false;
  if (outer.endLine === inner.endLine && outer.endCol < inner.endCol) return false;
  return true;
}

export { extractFallbackNodes, fallbackFunctionRanges, expandToBlock, indentOf };
