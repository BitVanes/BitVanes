import * as path from 'path';
import * as vscode from 'vscode';
import type { WalkthroughPlan, WalkthroughStep } from '../types/protocol';

export interface WalkthroughState {
  plan: WalkthroughPlan;
  current: number;
}

const MAX_HOVER_LEN = 1_200;

export class EditorDirector implements vscode.Disposable {
  private spotlight?: vscode.TextEditorDecorationType;
  private dim?: vscode.TextEditorDecorationType;
  private scope?: vscode.TextEditorDecorationType;
  private chip?: vscode.TextEditorDecorationType;
  private plan?: WalkthroughPlan;
  private current = -1;

  private readonly _onDidChangeState = new vscode.EventEmitter<WalkthroughState | null>();
  readonly onDidChangeState: vscode.Event<WalkthroughState | null> = this._onDidChangeState.event;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly getRoot: () => string | undefined) {
    this.rebuildDecorations();
    this.disposables.push(
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration('bitvanes.editor')) {
          this.rebuildDecorations();
          this.reapply();
        }
      }),
    );
  }

  get state(): WalkthroughState | null {
    if (!this.plan || this.current < 0) return null;
    return { plan: this.plan, current: this.current };
  }

  get currentStep(): WalkthroughStep | undefined {
    return this.plan?.steps[this.current];
  }

  start(plan: WalkthroughPlan): void {
    this.plan = plan;
    void vscode.commands.executeCommand('setContext', 'bitvanes.active', true);
    this.jump(0);
  }

  next(): void {
    this.jump(this.current + 1);
  }

  prev(): void {
    this.jump(this.current - 1);
  }

  jump(index: number): void {
    if (!this.plan || this.plan.steps.length === 0) return;
    const clamped = Math.min(Math.max(0, index), this.plan.steps.length - 1);
    const step = this.plan.steps[clamped];
    if (!step) return;
    this.current = clamped;
    void vscode.commands.executeCommand('setContext', 'bitvanes.stepIndex', clamped);
    void this.navigateToStep(step);
    this._onDidChangeState.fire(this.state);
  }

  exit(): void {
    this.plan = undefined;
    this.current = -1;
    this.clearAllDecorations();
    void vscode.commands.executeCommand('setContext', 'bitvanes.active', false);
    this._onDidChangeState.fire(null);
  }

  private async navigateToStep(step: WalkthroughStep): Promise<void> {
    const abs = this.resolveAbs(step.filePath);
    if (!abs) {
      void vscode.window.showWarningMessage(`BitVanes: could not resolve ${step.filePath} in this workspace.`);
      return;
    }
    try {
      const doc = await vscode.workspace.openTextDocument(abs);
      const editor = await vscode.window.showTextDocument(doc, {
        viewColumn: vscode.ViewColumn.Active,
        preserveFocus: false,
      });
      const vsRange = toVsRange(step.range, doc);
      const scopeVs = step.scopeRange ? toVsRange(step.scopeRange, doc) : undefined;
      editor.revealRange(scopeVs ?? vsRange, vscode.TextEditorRevealType.InCenter);
      this.applyDecorations(editor, step, vsRange, scopeVs);
    } catch {
      void vscode.window.showWarningMessage(`BitVanes: file not found for step — ${step.filePath}`);
    }
  }

  private applyDecorations(
    editor: vscode.TextEditor,
    step: WalkthroughStep,
    vsRange: vscode.Range,
    scopeVs: vscode.Range | undefined,
  ): void {
    for (const e of vscode.window.visibleTextEditors) {
      if (e === editor) continue;
      e.setDecorations(this.spotlight!, []);
      e.setDecorations(this.dim!, []);
      e.setDecorations(this.scope!, []);
      e.setDecorations(this.chip!, []);
    }
    editor.setDecorations(this.spotlight!, [{ range: vsRange, hoverMessage: this.stepHover(step) }]);

    const chipText = step.variable
      ? ` ← ${step.variable.name}${
          step.variable.stateBefore !== undefined || step.variable.stateAfter !== undefined
            ? `: ${shorten(step.variable.stateBefore ?? '?')} → ${shorten(step.variable.stateAfter ?? '?')}`
            : ''
        }${step.securityNote ? ' ⚠' : ''}`
      : step.securityNote
        ? ' ← ⚠ see security note'
        : undefined;
    if (chipText) {
      editor.setDecorations(this.chip!, [
        {
          range: new vscode.Range(vsRange.end.line, vsRange.end.character, vsRange.end.line, vsRange.end.character),
          renderOptions: { after: { contentText: chipText } },
        },
      ]);
    } else {
      editor.setDecorations(this.chip!, []);
    }

    const focus = scopeVs ?? vsRange;
    const doc = editor.document;
    const dimRanges: vscode.Range[] = [];
    if (focus.start.line > 0) {
      dimRanges.push(new vscode.Range(0, 0, focus.start.line, 0));
    }
    if (focus.end.line + 1 < doc.lineCount) {
      dimRanges.push(new vscode.Range(Math.min(focus.end.line + 1, doc.lineCount - 1), 0, doc.lineCount - 1, 0));
    }
    editor.setDecorations(this.dim!, dimRanges);

    if (scopeVs) {
      const scopeHover = new vscode.MarkdownString(undefined, true);
      scopeHover.supportThemeIcons = true;
      scopeHover.appendMarkdown(`**Scope** — enclosing function or block for this step.`);
      editor.setDecorations(this.scope!, [{ range: scopeVs, hoverMessage: scopeHover }]);
    } else {
      editor.setDecorations(this.scope!, []);
    }
  }

  private stepHover(step: WalkthroughStep): vscode.MarkdownString {
    const m = new vscode.MarkdownString(undefined, true);
    m.supportThemeIcons = true;
    const total = this.plan?.steps.length ?? 0;
    const icon = step.securityNote ? '$(alert)' : '$(symbol-misc)';
    m.appendMarkdown(`### ${icon} Step ${this.current + 1}/${total} — ${mdEscape(step.title)}\n\n`);
    m.appendMarkdown(step.explanation.slice(0, MAX_HOVER_LEN));
    m.appendMarkdown('\n\n');
    if (step.variable) {
      const v = step.variable;
      m.appendMarkdown(
        `**\`${mdInline(v.name)}\`** \`$(${actionIcon(v.action)}) ${v.action}\`` +
          (v.stateBefore !== undefined || v.stateAfter !== undefined
            ? ` — \`${mdInline(String(v.stateBefore ?? '?'))}\` → \`${mdInline(String(v.stateAfter ?? '?'))}\``
            : '') +
          '\n\n',
      );
    }
    if (step.securityNote) {
      m.appendMarkdown(`> $(alert) **Security:** ${mdEscape(step.securityNote)}\n\n`);
    }
    m.appendMarkdown(`---\n[◀ prev](command:bitvanes.prevStep) · [browse steps](command:bitvanes.explainStep) · [next ▶](command:bitvanes.nextStep)`);
    m.isTrusted = { enabledCommands: ['bitvanes.explainStep', 'bitvanes.nextStep', 'bitvanes.prevStep'] };
    return m;
  }

  private reapply(): void {
    const step = this.currentStep;
    if (!step) return;
    const editor = vscode.window.activeTextEditor;
    if (!editor) return;
    const doc = editor.document;
    const vsRange = toVsRange(step.range, doc);
    const scopeVs = step.scopeRange ? toVsRange(step.scopeRange, doc) : undefined;
    this.applyDecorations(editor, step, vsRange, scopeVs);
  }

  private clearAllDecorations(): void {
    for (const e of vscode.window.visibleTextEditors) {
      e.setDecorations(this.spotlight!, []);
      e.setDecorations(this.dim!, []);
      e.setDecorations(this.scope!, []);
      e.setDecorations(this.chip!, []);
    }
  }

  private resolveAbs(filePath: string): string | undefined {
    const norm = filePath.replace(/\\/g, '/');
    if (path.isAbsolute(norm)) return norm;
    const root = this.getRoot();
    if (!root) return undefined;
    return path.resolve(root, norm);
  }

  private rebuildDecorations(): void {
    this.spotlight?.dispose();
    this.dim?.dispose();
    this.scope?.dispose();

    const cfg = vscode.workspace.getConfiguration('bitvanes.editor');
    const color = normalizeHex(cfg.get<string>('spotlightColor', '#f59e0b')) ?? '#f59e0b';
    const dimOpacity = cfg.get<number>('dimOpacity', 0.3);

    this.spotlight = vscode.window.createTextEditorDecorationType({
      borderWidth: '1.5px',
      borderStyle: 'solid',
      borderColor: color,
      backgroundColor: withAlpha(color, 0.2),
      borderRadius: '3px',
      overviewRulerColor: color,
      overviewRulerLane: vscode.OverviewRulerLane.Center,
    });
    this.dim = vscode.window.createTextEditorDecorationType({
      opacity: String(dimOpacity),
      isWholeLine: true,
      rangeBehavior: vscode.DecorationRangeBehavior.ClosedOpen,
    });
    this.scope = vscode.window.createTextEditorDecorationType({
      isWholeLine: true,
      borderWidth: '1px',
      borderStyle: 'dotted',
      borderColor: 'var(--vscode-widget-border)',
      borderRadius: '4px',
    });
    this.chip = vscode.window.createTextEditorDecorationType({
      after: {
        fontStyle: 'italic',
        color: 'var(--vscode-editorInfo-foreground)',
      },
    });
    if (this.plan && this.current >= 0) {
      this.reapply();
    }
  }

  dispose(): void {
    this.clearAllDecorations();
    this.spotlight?.dispose();
    this.dim?.dispose();
    this.scope?.dispose();
    this.chip?.dispose();
    this._onDidChangeState.dispose();
    for (const d of this.disposables) d.dispose();
  }
}

function shorten(v: string): string {
  const t = v.replace(/\n/g, ' ').trim();
  return t.length > 32 ? `${t.slice(0, 31)}…` : t;
}

function toVsRange(r: { startLine: number; startCol: number; endLine: number; endCol: number }, doc: vscode.TextDocument): vscode.Range {
  const maxLine = doc.lineCount - 1;
  const startLine = Math.min(Math.max(0, r.startLine - 1), maxLine);
  const endLine = Math.min(Math.max(startLine, r.endLine - 1), maxLine);
  const startCol = Math.min(Math.max(0, r.startCol - 1), doc.lineAt(startLine).text.length);
  const endCol = Math.min(Math.max(0, r.endCol - 1), doc.lineAt(endLine).text.length);
  return new vscode.Range(startLine, startCol, endLine, Math.max(startLine === endLine ? startCol : 0, endCol));
}

function normalizeHex(hex: string): string | undefined {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!m || !m[1]) return undefined;
  const g = m[1];
  if (g.length === 3) {
    const [a = '', b = '', c = ''] = g.split('');
    return `#${a}${a}${b}${b}${c}${c}`.toLowerCase();
  }
  return `#${g}`.toLowerCase();
}

function withAlpha(hex: string, alpha: number): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

function mdEscape(s: string): string {
  return s.replace(/([\\`*_{}[\]()#+!~|<>])/g, '\\$1');
}

function mdInline(s: string): string {
  return s.replace(/`/g, "'").replace(/\n/g, ' ').slice(0, 120);
}

export function actionIcon(action: string): string {
  switch (action) {
    case 'init':
      return 'sparkle';
    case 'pass':
      return 'arrow-right';
    case 'validate':
      return 'shield';
    case 'mutate':
      return 'edit';
    case 'commit':
      return 'git-commit';
    case 'return':
      return 'debug-return';
    default:
      return 'symbol-variable';
  }
}
