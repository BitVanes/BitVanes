import { beforeAll, describe, expect, it } from 'vitest';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { TreeSitterResolver } from '../src/ast/TreeSitterResolver';

const RES = join(process.cwd(), 'resources');
const haveGrammars = existsSync(join(RES, 'grammars', 'tree-sitter-rust.wasm'));

const RUST = `fn transfer(from: &mut Account, amount: u64) -> bool {
    require(amount > 0, "zero");
    let before = from.balance;
    from.balance = before - amount;
    from.count += 1;
    true
}
`;

const SOL = `contract Vault {
    mapping(address => uint256) public balances;

    function deposit() external payable {
        require(msg.value > 0, "zero");
        uint256 before = balances[msg.sender];
        balances[msg.sender] = before + msg.value;
        emit Deposited(msg.sender, msg.value);
    }
}
`;

const TS = `export function transfer(from: Account, amount: number): void {
    if (amount <= 0) throw new Error("zero");
    const before = from.balance;
    from.balance = before - amount;
}
`;

describe.skipIf(!haveGrammars)('TreeSitterResolver (real grammars)', () => {
  let resolver: TreeSitterResolver;

  beforeAll(() => {
    resolver = new TreeSitterResolver(RES);
  });

  it('parses rust and categorizes nodes', async () => {
    const ast = await resolver.parse('/x/src/vault.rs', RUST);
    expect(ast.fallback).toBe(false);
    expect(ast.language).toBe('rust');
    expect(ast.nodes.some((n) => n.category === 'function')).toBe(true);
    expect(ast.nodes.some((n) => n.category === 'assignment')).toBe(true);
    expect(ast.nodes.some((n) => n.category === 'call')).toBe(true);
  });

  it('parses solidity functions and emits', async () => {
    const ast = await resolver.parse('/x/Vault.sol', SOL);
    expect(ast.fallback).toBe(false);
    const fns = ast.nodes.filter((n) => n.category === 'function');
    expect(fns.some((n) => n.text.includes('deposit'))).toBe(true);
    expect(ast.nodes.some((n) => n.category === 'security' && n.type === 'emit_statement')).toBe(true);
    expect(ast.nodes.some((n) => n.category === 'call')).toBe(true);
  });

  it('parses typescript', async () => {
    const ast = await resolver.parse('/x/transfer.ts', TS);
    expect(ast.fallback).toBe(false);
    expect(ast.nodes.some((n) => n.category === 'function')).toBe(true);
    expect(ast.nodes.some((n) => n.category === 'declaration')).toBe(true);
  });

  it('falls back for unsupported extensions without throwing', async () => {
    const ast = await resolver.parse('/x/data.csv', 'a,b,c\n1,2,3\nx=4');
    expect(ast.fallback).toBe(true);
    expect(ast.nodes.length).toBeGreaterThan(0);
  });

  it('falls back for malformed supported sources without throwing', async () => {
    const ast = await resolver.parse('/x/broken.rs', 'fn ((( {\n\tlet = =');
    expect(ast).toBeTruthy();
  });

  it('finds the enclosing scope for a line', async () => {
    const ast = await resolver.parse('/x/Vault.sol', SOL);
    const scope = resolver.enclosingScope(ast, SOL, 6);
    expect(scope).toBeDefined();
    expect(scope!.range.startLine).toBe(4);
    expect(scope!.range.endLine).toBe(9);
  });

  it('snaps a slightly misaligned range onto a statement', async () => {
    const ast = await resolver.parse('/x/src/vault.rs', RUST);
    const snapped = resolver.snapRange(ast, RUST, { startLine: 4, startCol: 1, endLine: 4, endCol: 999 });
    expect(snapped.startLine).toBe(4);
    const line = RUST.split('\n')[3] ?? '';
    expect(snapped.startCol).toBe(line.length - line.trimStart().length + 1);
    expect(snapped.endCol).toBeLessThanOrEqual(line.trimEnd().length + 1);
  });

  it('keeps a range that already matches a node', async () => {
    const ast = await resolver.parse('/x/src/vault.rs', RUST);
    const assignment = ast.nodes.find((n) => n.category === 'assignment')!;
    const snapped = resolver.snapRange(ast, RUST, assignment.range);
    expect(snapped.startLine).toBe(assignment.range.startLine);
  });

  it('clamps ranges beyond the document', async () => {
    const ast = await resolver.parse('/x/src/vault.rs', RUST);
    const snapped = resolver.snapRange(ast, RUST, { startLine: 100, startCol: 1, endLine: 200, endCol: 1 });
    expect(snapped.endLine).toBeLessThanOrEqual(RUST.split('\n').length);
  });
});
