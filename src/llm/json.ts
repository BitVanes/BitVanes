export function extractJson(text: string): unknown {
  let body = text.trim();
  const fenced = /```(?:json)?\s*([\s\S]*?)```/.exec(body);
  if (fenced?.[1] && fenced[1].includes('{')) {
    body = fenced[1].trim();
  }

  const start = body.indexOf('{');
  if (start < 0) throw new Error('no JSON object found in model output');
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let i = start; i < body.length; i++) {
    const ch = body[i];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (ch === '\\') {
      if (inString) escaped = true;
      continue;
    }
    if (ch === '"') {
      inString = !inString;
      continue;
    }
    if (inString) continue;
    if (ch === '{' || ch === '[') depth++;
    else if (ch === '}' || ch === ']') {
      depth--;
      if (depth === 0 && ch === '}') {
        const slice = body.slice(start, i + 1);
        try {
          return JSON.parse(slice);
        } catch {
          return JSON.parse(stripTrailingCommas(slice));
        }
      }
    }
  }
  throw new Error('unbalanced JSON in model output');
}

function stripTrailingCommas(s: string): string {
  return s.replace(/,(\s*[}\]])/g, '$1');
}
