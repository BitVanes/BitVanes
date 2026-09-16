import * as vscode from 'vscode';
import type { WalkthroughState } from '../director/EditorDirector';
import { mdEscape, mdInline } from '../util/markdown';

export class StatusBarController implements vscode.Disposable {
  private readonly prev: vscode.StatusBarItem;
  private readonly counter: vscode.StatusBarItem;
  private readonly next: vscode.StatusBarItem;
  private readonly autoplay: vscode.StatusBarItem;
  private readonly exit: vscode.StatusBarItem;
  private autoplayRunning = false;

  constructor() {
    this.prev = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 102);
    this.prev.command = 'bitvanes.prevStep';
    this.prev.text = '$(chevron-left)';
    this.prev.tooltip = 'BitVanes: previous step ([)';
    this.prev.name = 'BitVanes';

    this.counter = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 101);
    this.counter.command = 'bitvanes.explainStep';
    this.counter.name = 'BitVanes';
    this.counter.backgroundColor = new vscode.ThemeColor('statusBarItem.prominentBackground');

    this.next = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    this.next.command = 'bitvanes.nextStep';
    this.next.text = '$(chevron-right)';
    this.next.tooltip = 'BitVanes: next step (])';
    this.next.name = 'BitVanes';

    this.autoplay = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 99);
    this.autoplay.command = 'bitvanes.toggleAutoplay';
    this.autoplay.text = '$(play)';
    this.autoplay.tooltip = 'BitVanes: toggle autoplay';
    this.autoplay.name = 'BitVanes';

    this.exit = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 98);
    this.exit.command = 'bitvanes.exitWalkthrough';
    this.exit.text = '$(close)';
    this.exit.tooltip = 'BitVanes: exit walkthrough (Esc)';
    this.exit.name = 'BitVanes';
  }

  update(state: WalkthroughState | null): void {
    if (!state) {
      this.hide();
      return;
    }
    this.counter.text = `BitVanes $(list-ordered) ${state.current + 1}/${state.plan.totalSteps}`;
    this.counter.tooltip = new vscode.MarkdownString(
      `**${mdEscape(state.plan.summary.replace(/\n/g, ' '))}**\n\nEntry: \`${mdInline(state.plan.entryPoint)}\``,
    );
    this.prev.show();
    this.counter.show();
    this.next.show();
    this.autoplay.show();
    this.exit.show();
  }

  setAutoplay(running: boolean): void {
    this.autoplayRunning = running;
    this.autoplay.text = running ? '$(debug-pause)' : '$(play)';
    this.autoplay.tooltip = running ? 'BitVanes: stop autoplay' : 'BitVanes: start autoplay';
  }

  get isAutoplayRunning(): boolean {
    return this.autoplayRunning;
  }

  private hide(): void {
    this.prev.hide();
    this.counter.hide();
    this.next.hide();
    this.autoplay.hide();
    this.exit.hide();
  }

  dispose(): void {
    this.prev.dispose();
    this.counter.dispose();
    this.next.dispose();
    this.autoplay.dispose();
    this.exit.dispose();
  }
}
