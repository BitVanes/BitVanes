import { describe, expect, it } from 'vitest';
import { extractJson } from '../src/llm/json';

describe('extractJson', () => {
  it('parses plain JSON', () => {
    expect(extractJson('{"a":1}')).toEqual({ a: 1 });
  });

  it('parses JSON in code fences', () => {
    expect(extractJson('Here is the plan:\n```json\n{"a": {"b": 2}}\n```')).toEqual({ a: { b: 2 } });
  });

  it('parses JSON wrapped in prose with trailing junk', () => {
    expect(extractJson('Sure! {"steps": []} hope that helps')).toEqual({ steps: [] });
  });

  it('tolerates braces inside strings', () => {
    expect(extractJson('{"note": "contains } brace"}')).toEqual({ note: 'contains } brace' });
  });

  it('tolerates escaped quotes inside strings', () => {
    expect(extractJson('{"note": "say \\"hi\\""}')).toEqual({ note: 'say "hi"' });
  });

  it('strips trailing commas as a repair pass', () => {
    expect(extractJson('{"a": 1, "b": [1, 2,],}')).toEqual({ a: 1, b: [1, 2] });
  });

  it('throws when no object exists', () => {
    expect(() => extractJson('no json here')).toThrow(/no JSON object/);
  });

  it('throws on unbalanced JSON', () => {
    expect(() => extractJson('{"a": 1')).toThrow(/unbalanced/);
  });
});
