import { describe, expect, it } from 'vitest';
import { extractFallbackNodes, fallbackFunctionRanges } from '../src/ast/LineFallback';
import { expandToBlock } from '../src/util/text';

const RUST = `use std::fmt;

pub fn transfer(from: &mut Account, amount: u64) -> bool {
    require(amount > 0, "zero");
    let before = from.balance;
    from.balance = before - amount;
    from.total += 1;
    emit Transfer(from.id, amount);
    true
}

pub struct Account {
    pub balance: u64,
}
`;

describe('extractFallbackNodes', () => {
  const nodes = extractFallbackNodes(RUST);

  it('finds function declarations', () => {
    const fns = nodes.filter((n) => n.category === 'function');
    expect(fns.some((n) => n.text === 'transfer')).toBe(true);
    expect(fns.some((n) => n.text === 'Account')).toBe(true);
  });

  it('finds assignments and mutations', () => {
    const assigns = nodes.filter((n) => n.category === 'assignment');
    expect(assigns.some((n) => n.text.startsWith('before ='))).toBe(true);
    expect(assigns.some((n) => n.text.startsWith('from.total'))).toBe(true);
  });

  it('finds calls but skips keywords', () => {
    const calls = nodes.filter((n) => n.category === 'call');
    expect(calls.some((n) => n.text.startsWith('require('))).toBe(true);
    expect(calls.some((n) => n.text.startsWith('Transfer('))).toBe(true);
    expect(calls.some((n) => n.text === 'if(…)')).toBe(false);
  });

  it('flags control and validation lines', () => {
    expect(nodes.some((n) => n.category === 'control' && n.text.includes('require'))).toBe(true);
  });
});

describe('fallbackFunctionRanges', () => {
  it('derives function spans by indentation', () => {
    const fns = fallbackFunctionRanges(RUST);
    const transfer = fns.find((f) => f.name === 'transfer');
    expect(transfer?.startLine).toBe(3);
    expect(transfer?.endLine).toBe(9);
  });
});

describe('expandToBlock', () => {
  const PY = [
    'def outer():',
    '    x = 1',
    '    if x:',
    '        y = 2',
    '    return x',
    '',
    'def other():',
    '    pass',
  ];

  it('expands from an inner statement to the enclosing block plus header', () => {
    const block = expandToBlock(PY, 3);
    expect(block.startLine).toBe(2);
    expect(block.endLine).toBe(3);
  });

  it('expands from cursor to whole function', () => {
    const block = expandToBlock(PY, 1);
    expect(block.startLine).toBe(0);
    expect(block.endLine).toBe(4);
  });

  it('does not bleed into the next definition', () => {
    const block = expandToBlock(PY, 7);
    expect(block.endLine).toBe(7);
  });
});
