export interface Range {
  startLine: number;
  startCol: number;
  endLine: number;
  endCol: number;
}

export interface VariableMutation {
  name: string;
  action: 'init' | 'pass' | 'validate' | 'mutate' | 'commit' | 'return';
  stateBefore?: string;
  stateAfter?: string;
}

export interface WalkthroughStep {
  stepIndex: number;
  title: string;
  filePath: string;
  range: Range;
  scopeRange?: Range;
  variable?: VariableMutation;
  explanation: string;
  securityNote?: string;
}

export interface WalkthroughPlan {
  summary: string;
  entryPoint: string;
  totalSteps: number;
  steps: WalkthroughStep[];
}

export interface PlanIssue {
  step?: number;
  field: string;
  message: string;
}

export class PlanValidationError extends Error {
  constructor(message: string, readonly issues: PlanIssue[]) {
    super(message);
    this.name = 'PlanValidationError';
  }
}

const VALID_ACTIONS = new Set(['init', 'pass', 'validate', 'mutate', 'commit', 'return']);

const ACTION_ALIASES: Record<string, VariableMutation['action']> = {
  initialize: 'init',
  initialized: 'init',
  declare: 'init',
  declared: 'init',
  declaration: 'init',
  create: 'init',
  created: 'init',
  assign: 'init',
  assigned: 'init',
  parameter: 'pass',
  param: 'pass',
  argument: 'pass',
  args: 'pass',
  input: 'pass',
  receive: 'pass',
  received: 'pass',
  propagate: 'pass',
  passthrough: 'pass',
  check: 'validate',
  checked: 'validate',
  verify: 'validate',
  verified: 'validate',
  assert: 'validate',
  assertion: 'validate',
  validated: 'validate',
  guard: 'validate',
  update: 'mutate',
  updated: 'mutate',
  modify: 'mutate',
  modified: 'mutate',
  change: 'mutate',
  changed: 'mutate',
  set: 'mutate',
  transform: 'mutate',
  transformed: 'mutate',
  calculation: 'mutate',
  compute: 'mutate',
  computed: 'mutate',
  save: 'commit',
  saved: 'commit',
  persist: 'commit',
  persisted: 'commit',
  store: 'commit',
  stored: 'commit',
  write: 'commit',
  written: 'commit',
  emit: 'commit',
  published: 'commit',
  returns: 'return',
  returned: 'return',
  output: 'return',
  result: 'return',
};

const MAX_LINES = 5_000_000;
const MAX_COLS = 100_000;

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function num(v: unknown): number | undefined {
  if (typeof v === 'number' && Number.isFinite(v)) return v;
  if (typeof v === 'string' && v.trim() !== '') {
    const n = Number(v);
    if (Number.isFinite(n)) return n;
  }
  return undefined;
}

function str(v: unknown): string | undefined {
  if (typeof v === 'string') return v;
  if (typeof v === 'number' || typeof v === 'boolean') return String(v);
  return undefined;
}

function coerceAction(v: unknown): VariableMutation['action'] | undefined {
  if (typeof v !== 'string') return undefined;
  const a = v.trim().toLowerCase();
  if (VALID_ACTIONS.has(a as VariableMutation['action'])) {
    return a as VariableMutation['action'];
  }
  const alias = ACTION_ALIASES[a];
  if (alias) return alias;
  if (a.includes('valid')) return 'validate';
  if (a.includes('mutat') || a.includes('modif') || a.includes('updat')) return 'mutate';
  if (a.includes('commit') || a.includes('persist') || a.includes('sav') || a.includes('stor')) return 'commit';
  if (a.includes('return')) return 'return';
  if (a.includes('init') || a.includes('declar') || a.includes('creat')) return 'init';
  if (a.includes('pass') || a.includes('arg') || a.includes('param')) return 'pass';
  return undefined;
}

export function coerceRange(raw: unknown, issues: PlanIssue[], field: string): Range | undefined {
  if (!isObject(raw)) {
    issues.push({ field, message: 'range must be an object' });
    return undefined;
  }
  let startLine = num(raw['startLine']);
  let endLine = num(raw['endLine']);
  const startColRaw = num(raw['startCol']);
  const endColRaw = num(raw['endCol']);

  if (startLine === undefined || endLine === undefined) {
    issues.push({ field, message: 'range requires numeric startLine and endLine' });
    return undefined;
  }

  // A 0 startLine is the only unambiguous evidence of 0-based lines; shift the
  // lines. Columns are handled independently below so that 0-based columns
  // (a very common partial confusion) never shift the line numbers.
  if (startLine === 0) {
    startLine += 1;
    endLine += 1;
    issues.push({ field, message: '0-based line coordinates detected; normalized to 1-based' });
  } else if (endLine < 1) {
    // e.g. {startLine: 10, endLine: 0} — collapse instead of inventing a span.
    endLine = startLine;
    issues.push({ field, message: 'endLine below 1; collapsed to startLine' });
  }

  // startCol clamps to column 1; an omitted endCol means "to end of line" and
  // uses the end-of-line sentinel (later clamped to the real line length).
  if (startColRaw === 0 || endColRaw === 0) {
    issues.push({ field, message: '0-based column detected; clamped to 1' });
  }
  const startCol = Math.max(1, startColRaw ?? 1);
  const endCol = Math.max(1, endColRaw ?? MAX_COLS);

  startLine = Math.max(1, Math.min(startLine, MAX_LINES));
  endLine = Math.max(1, Math.min(endLine, MAX_LINES));
  let safeStartCol = Math.min(startCol, MAX_COLS);
  let safeEndCol = Math.min(endCol, MAX_COLS);

  if (endLine < startLine) {
    [startLine, endLine] = [endLine, startLine];
  }
  if (endLine === startLine && safeEndCol < safeStartCol) {
    [safeStartCol, safeEndCol] = [safeEndCol, safeStartCol];
  }
  if ((startColRaw !== undefined && startColRaw >= MAX_COLS) || (endColRaw !== undefined && endColRaw >= MAX_COLS)) {
    issues.push({ field, message: 'suspiciously large column values; end-of-line assumed' });
  }

  return { startLine, startCol: safeStartCol, endLine, endCol: safeEndCol };
}

function coerceVariable(raw: unknown, issues: PlanIssue[], field: string): VariableMutation | undefined {
  if (!isObject(raw)) {
    issues.push({ field, message: 'variable must be an object' });
    return undefined;
  }
  const name = str(raw['name'])?.trim();
  if (!name) {
    issues.push({ field, message: 'variable.name missing' });
    return undefined;
  }
  const action = coerceAction(raw['action']);
  if (!action) {
    issues.push({ field, message: `variable.action "${String(raw['action'])}" not recognized; step kept without variable detail` });
    return undefined;
  }
  const out: VariableMutation = { name, action };
  const before = str(raw['stateBefore']);
  const after = str(raw['stateAfter']);
  if (before !== undefined) out.stateBefore = before;
  if (after !== undefined) out.stateAfter = after;
  return out;
}

function normalizePath(p: string): string {
  return p.replace(/\\/g, '/').replace(/^\.\//, '').replace(/^a\//, '').replace(/^b\//, '');
}

export function validateWalkthroughPlan(
  raw: unknown,
  opts: { maxSteps: number; defaultFilePath?: string },
): WalkthroughPlan {
  const issues: PlanIssue[] = [];

  if (!isObject(raw)) {
    throw new PlanValidationError('Model output was not a JSON object', [
      { field: 'root', message: `expected object, got ${raw === null ? 'null' : typeof raw}` },
    ]);
  }

  const rawSteps = Array.isArray(raw['steps']) ? raw['steps'] : [];
  if (rawSteps.length === 0) {
    issues.push({ field: 'steps', message: 'no steps produced' });
  }

  const steps: WalkthroughStep[] = [];
  for (let i = 0; i < rawSteps.length && steps.length < opts.maxSteps; i++) {
    const s = rawSteps[i];
    if (!isObject(s)) {
      issues.push({ step: i, field: 'steps[]', message: 'step is not an object; dropped' });
      continue;
    }
    const title = str(s['title'])?.trim() || `Step ${steps.length + 1}`;
    const filePathRaw = str(s['filePath'])?.trim();
    const filePath = filePathRaw ? normalizePath(filePathRaw) : opts.defaultFilePath;
    if (!filePath) {
      issues.push({ step: i, field: 'filePath', message: 'filePath missing and no default; step dropped' });
      continue;
    }
    const range = coerceRange(s['range'], issues, `steps[${i}].range`);
    if (!range) {
      issues.push({ step: i, field: 'range', message: 'invalid range; step dropped' });
      continue;
    }
    const scopeRange = s['scopeRange'] === undefined ? undefined : coerceRange(s['scopeRange'], issues, `steps[${i}].scopeRange`);
    const explanation = str(s['explanation'])?.trim();
    if (!explanation) {
      issues.push({ step: i, field: 'explanation', message: 'explanation missing; step dropped' });
      continue;
    }
    const variable = s['variable'] === undefined ? undefined : coerceVariable(s['variable'], issues, `steps[${i}].variable`);
    const securityNote = str(s['securityNote'])?.trim();

    steps.push({
      stepIndex: steps.length,
      title,
      filePath,
      range,
      ...(scopeRange ? { scopeRange } : {}),
      ...(variable ? { variable } : {}),
      explanation,
      ...(securityNote ? { securityNote } : {}),
    });
  }

  if (steps.length === 0) {
    throw new PlanValidationError('No valid walkthrough steps could be recovered from model output', issues);
  }

  const first = steps[0]!;
  const summary = str(raw['summary'])?.trim() || `Walkthrough of ${first.filePath}`;
  const entryPoint = str(raw['entryPoint'])?.trim() || first.filePath;

  return {
    summary,
    entryPoint,
    totalSteps: steps.length,
    steps,
  };
}
