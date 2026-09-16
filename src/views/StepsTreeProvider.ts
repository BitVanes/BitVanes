import * as vscode from 'vscode';
import type { WalkthroughPlan, WalkthroughStep } from '../types/protocol';
import type { WalkthroughState } from '../director/EditorDirector';
import { actionIcon } from '../director/EditorDirector';
import { toDisplayPath } from '../util/text';

type TreeNode =
  | { kind: 'summary'; plan: WalkthroughPlan }
  | { kind: 'step'; step: WalkthroughStep; index: number; current: boolean };

export class StepsTreeProvider implements vscode.TreeDataProvider<TreeNode> {
  private readonly _onDidChangeTreeData = new vscode.EventEmitter<TreeNode | undefined>();
  readonly onDidChangeTreeData: vscode.Event<TreeNode | undefined> = this._onDidChangeTreeData.event;
  private state: WalkthroughState | null = null;

  update(state: WalkthroughState | null): void {
    this.state = state;
    this._onDidChangeTreeData.fire(undefined);
  }

  getTreeItem(node: TreeNode): vscode.TreeItem {
    if (node.kind === 'summary') {
      const item = new vscode.TreeItem('Walkthrough', vscode.TreeItemCollapsibleState.Expanded);
      item.iconPath = new vscode.ThemeIcon('route');
      item.description = truncate(node.plan.summary.replace(/\n/g, ' '), 60);
      const tip = new vscode.MarkdownString(undefined, true);
      tip.supportThemeIcons = true;
      tip.appendMarkdown(`**${mdEscape(node.plan.summary)}\n\n**Entry point:** \`$(${actionIcon('pass')}) ${mdInline(node.plan.entryPoint)}\`\n\n**Steps:** ${node.plan.totalSteps}`);
      item.tooltip = tip;
      item.contextValue = 'bitvanesSummary';
      return item;
    }

    const { step, index, current } = node;
    const item = new vscode.TreeItem(`${index + 1}. ${step.title}`, vscode.TreeItemCollapsibleState.None);
    const icon = current ? 'play' : step.securityNote ? 'alert' : step.variable ? actionIcon(step.variable.action) : 'circle-filled';
    const color =
      current
        ? new vscode.ThemeColor('charts.blue')
        : step.securityNote
          ? new vscode.ThemeColor('list.warningForeground')
          : undefined;
    item.iconPath = color ? new vscode.ThemeIcon(icon, color) : new vscode.ThemeIcon(icon, new vscode.ThemeColor('foreground'));
    item.description = [
      step.variable ? `${step.variable.name} (${step.variable.action})` : '',
      toDisplayPath(step.filePath),
    ]
      .filter(Boolean)
      .join(' · ');
    item.command = { command: 'bitvanes.jumpToStep', title: 'Jump to step', arguments: [index] };
    item.tooltip = this.stepTooltip(step, index, current);
    item.contextValue = 'bitvanesStep';
    return item;
  }

  getChildren(element?: TreeNode): TreeNode[] {
    if (!this.state) return [];
    if (!element) {
      return [{ kind: 'summary', plan: this.state.plan }];
    }
    if (element.kind === 'summary') {
      return this.state.plan.steps.map((step, index) => ({
        kind: 'step' as const,
        step,
        index,
        current: index === this.state?.current,
      }));
    }
    return [];
  }

  findStepNode(index: number): TreeNode | undefined {
    if (!this.state) return undefined;
    const step = this.state.plan.steps[index];
    if (!step) return undefined;
    return { kind: 'step', step, index, current: index === this.state.current };
  }

  private stepTooltip(step: WalkthroughStep, index: number, current: boolean): vscode.MarkdownString {
    const m = new vscode.MarkdownString(undefined, true);
    m.supportThemeIcons = true;
    m.appendMarkdown(`### ${current ? '$(play) ' : ''}${index + 1}. ${mdEscape(step.title)}\n\n`);
    m.appendMarkdown(step.explanation);
    m.appendMarkdown('\n\n');
    m.appendMarkdown(`\`${toDisplayPath(step.filePath)}:${step.range.startLine}-${step.range.endLine}\``);
    if (step.variable) {
      const v = step.variable;
      m.appendMarkdown(`\n\n**\`${mdInline(v.name)}\`** \`$(${actionIcon(v.action)}) ${v.action}\``);
      if (v.stateBefore !== undefined || v.stateAfter !== undefined) {
        m.appendMarkdown(` — \`${mdInline(String(v.stateBefore ?? '?'))}\` → \`${mdInline(String(v.stateAfter ?? '?'))}\``);
      }
    }
    if (step.securityNote) {
      m.appendMarkdown(`\n\n> $(alert) **Security:** ${mdEscape(step.securityNote)}`);
    }
    return m;
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s;
}

function mdEscape(s: string): string {
  return s.replace(/([\\`*_{}[\]()#+!~|<>])/g, '\\$1');
}

function mdInline(s: string): string {
  return s.replace(/`/g, "'").replace(/\n/g, ' ').slice(0, 120);
}
