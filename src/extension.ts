import * as fs from 'fs/promises';
import * as path from 'path';
import * as vscode from 'vscode';
import { TreeSitterResolver } from './ast/TreeSitterResolver';
import { EditorDirector } from './director/EditorDirector';
import { GitProvider, getActiveSelection } from './git/GitProvider';
import { LlmManager } from './llm/LlmManager';
import type { WalkthroughStyle } from './llm/prompts';
import { PlanValidationError } from './types/protocol';
import { buildDiffRequest, buildSelectionRequest, refinePlanRanges } from './pipeline/contextBuilder';
import { buildLocalPlan } from './pipeline/localPlan';
import { StatusBarController } from './views/StatusBarController';
import { WalkthroughSidebarProvider } from './views/WalkthroughSidebarProvider';
import { toDisplayPath } from './util/text';

class UserError extends Error {}

function normalizeStyle(v: unknown): WalkthroughStyle {
  return v === 'expert' || v === 'learner' ? v : 'standard';
}

let director: EditorDirector | undefined;
let autoplayTimer: ReturnType<typeof setInterval> | null = null;

export function activate(context: vscode.ExtensionContext): void {
  const resolver = new TreeSitterResolver(path.join(context.extensionPath, 'resources'));
  const llm = new LlmManager(context.secrets, context.globalState);
  director = new EditorDirector(GitProvider.workspaceRoot);

  const statusbar = new StatusBarController();
  const sidebar = new WalkthroughSidebarProvider(context.extensionUri, {
    next: () => {
      stopAutoplay();
      director?.next();
    },
    prev: () => {
      stopAutoplay();
      director?.prev();
    },
    jump: (i) => {
      stopAutoplay();
      director?.jump(i);
    },
    toggleAutoplay: () => {
      if (autoplayTimer) stopAutoplay();
      else if (director?.state) startAutoplay();
    },
    exit: () => {
      stopAutoplay();
      director?.exit();
    },
    browse: () => {
      void browseSteps();
    },
    start: (mode) => {
      void runPipeline(mode);
    },
    setStyle: (s) => {
      if (s !== 'expert' && s !== 'standard' && s !== 'learner') return;
      void vscode.workspace
        .getConfiguration('bitvanes.walkthrough')
        .update('style', s, vscode.ConfigurationTarget.Global);
      sidebar.setStyle(s);
    },
  });
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(WalkthroughSidebarProvider.viewId, sidebar, {
      webviewOptions: { retainContextWhenHidden: true },
    }),
  );
  sidebar.setStyle(normalizeStyle(vscode.workspace.getConfiguration('bitvanes.walkthrough').get<string>('style')));

  const stopAutoplay = () => {
    if (autoplayTimer) {
      clearInterval(autoplayTimer);
      autoplayTimer = null;
    }
    statusbar.setAutoplay(false);
    sidebar.setAutoplay(false);
  };
  const startAutoplay = () => {
    stopAutoplay();
    const interval = vscode.workspace.getConfiguration('bitvanes.autoplay').get<number>('intervalMs', 4000);
    statusbar.setAutoplay(true);
    sidebar.setAutoplay(true);
    autoplayTimer = setInterval(() => {
      const st = director?.state;
      if (!st) {
        stopAutoplay();
        return;
      }
      if (st.current >= st.plan.totalSteps - 1) {
        stopAutoplay();
        void vscode.window.setStatusBarMessage('BitVanes: walkthrough complete.', 4000);
        return;
      }
      director?.next();
    }, Math.max(1000, interval));
  };

  director.onDidChangeState((st) => {
    statusbar.update(st);
    sidebar.setState(st);
    if (!st) stopAutoplay();
  });

  const readText = (absPath: string) => fs.readFile(absPath, 'utf8');

  const runPipeline = async (kind: 'diff' | 'staged' | 'selection' | 'instant') => {
    const t0 = Date.now();
    try {
      await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, cancellable: true, title: 'BitVanes: generating walkthrough…' },
        async (progress, token) => {
          const root = GitProvider.workspaceRoot();

          if (kind === 'instant') {
            if (!root) throw new UserError('Open a folder to walkthrough its changes.');
            progress.report({ message: 'reading diff…' });
            const files = await GitProvider.diff(root, false);
            if (files.length === 0) throw new UserError('No working-tree changes to walkthrough.');
            progress.report({ message: 'analyzing syntax trees…' });
            const plan = await buildLocalPlan(
              files,
              resolver,
              readText,
              root,
              vscode.workspace.getConfiguration('bitvanes.llm').get('maxSteps', 12),
            );
            sidebar.reveal();
            director?.start(plan);
            const secs = ((Date.now() - t0) / 1000).toFixed(1);
            void vscode.window.showInformationMessage(
              `BitVanes: ${plan.totalSteps}-step instant walkthrough in ${secs}s (local, no model).`,
            );
            return;
          }

          let request;
          if (kind === 'selection') {
            const sel = getActiveSelection();
            if (!sel) throw new UserError('Open a file and place the cursor in (or select) the code to walkthrough.');
            request = await buildSelectionRequest(sel, resolver);
          } else {
            if (!root) throw new UserError('Open a folder to walkthrough its changes.');
            if (!(await GitProvider.isGitRepo(root))) throw new UserError('This workspace is not a Git repository.');
            progress.report({ message: 'reading diff…' });
            const files = await GitProvider.diff(root, kind === 'staged');
            if (files.length === 0) {
              throw new UserError(kind === 'staged' ? 'No staged changes to walkthrough.' : 'No working-tree changes to walkthrough.');
            }
            const contextLines = vscode.workspace.getConfiguration('bitvanes.llm').get<number>('contextLines', 6);
            request = await buildDiffRequest(files, resolver, readText, contextLines, root);
          }

          progress.report({ message: 'asking the model…' });
          const cfg = vscode.workspace.getConfiguration('bitvanes.llm');
          const style = normalizeStyle(vscode.workspace.getConfiguration('bitvanes.walkthrough').get<string>('style'));
          const { plan, provider } = await llm.generatePlan(
            request,
            {
              maxSteps: cfg.get('maxSteps', 12),
              maxTokens: cfg.get('maxTokens', 4096),
              temperature: cfg.get('temperature', 0.2),
              style,
            },
            token,
          );

          if (root) {
            progress.report({ message: 'aligning steps to syntax…' });
            await refinePlanRanges(plan, resolver, root, readText);
          }

          sidebar.reveal();
          director?.start(plan);
          const secs = ((Date.now() - t0) / 1000).toFixed(1);
          void vscode.window.showInformationMessage(
            `BitVanes: ${plan.totalSteps}-step walkthrough in ${secs}s via ${provider}. Hover the highlight or press alt+] to step.`,
          );
        },
      );
    } catch (err) {
      if (err instanceof Error && /cancel/i.test(err.message)) return;
      const msg = err instanceof UserError || err instanceof Error ? err.message : String(err);
      if (err instanceof PlanValidationError) {
        void vscode.window.showErrorMessage(`BitVanes: model output invalid — ${err.message}`);
      } else {
        void vscode.window.showErrorMessage(`BitVanes: ${msg}`);
      }
    }
  };

  const register = (id: string, cb: (...args: unknown[]) => unknown) => {
    context.subscriptions.push(vscode.commands.registerCommand(id, cb));
  };

  register('bitvanes.walkthroughDiff', () => runPipeline('diff'));
  register('bitvanes.walkthroughStagedDiff', () => runPipeline('staged'));
  register('bitvanes.walkthroughSelection', () => runPipeline('selection'));
  register('bitvanes.walkthroughInstant', () => runPipeline('instant'));
  register('bitvanes.setStyle', async () => {
    const styles: Array<{ id: WalkthroughStyle; label: string; description: string }> = [
      { id: 'expert', label: 'Expert', description: 'Senior engineers — terse, high-signal, invariants and risk only' },
      { id: 'standard', label: 'Standard', description: 'Working developers — what the code does and why' },
      { id: 'learner', label: 'Learner', description: 'New devs & vibe coders — plain language, syntax explained, jargon defined' },
    ];
    const current = normalizeStyle(vscode.workspace.getConfiguration('bitvanes.walkthrough').get<string>('style'));
    const picked = await vscode.window.showQuickPick(
      styles.map((s) => ({ ...s, label: `${s.id === current ? '$(check) ' : ''}${s.label}`, description: s.description })),
      { placeHolder: 'Who is the walkthrough for?' },
    );
    if (!picked) return;
    await vscode.workspace.getConfiguration('bitvanes.walkthrough').update('style', picked.id, vscode.ConfigurationTarget.Global);
    sidebar.setStyle(picked.id);
    void vscode.window.showInformationMessage(`BitVanes: walkthrough style set to ${picked.label}.`);
  });
  register('bitvanes.nextStep', () => {
    stopAutoplay();
    director?.next();
  });
  register('bitvanes.prevStep', () => {
    stopAutoplay();
    director?.prev();
  });
  register('bitvanes.jumpToStep', (index?: unknown) => {
    if (typeof index === 'number') {
      stopAutoplay();
      director?.jump(index);
    } else {
      void browseSteps();
    }
  });
  register('bitvanes.explainStep', () => browseSteps());
  register('bitvanes.toggleAutoplay', () => {
    if (autoplayTimer) stopAutoplay();
    else if (director?.state) startAutoplay();
  });
  register('bitvanes.exitWalkthrough', () => {
    stopAutoplay();
    director?.exit();
  });

  register('bitvanes.setApiKey', async () => {
    const presets: Array<{ label: string; description: string; url: string; model: string }> = [
      { label: 'OpenAI', description: 'https://api.openai.com/v1 — gpt-4o family', url: 'https://api.openai.com/v1', model: 'gpt-4o' },
      { label: 'Anthropic', description: 'https://api.anthropic.com/v1 — Claude family', url: 'https://api.anthropic.com/v1', model: 'claude-sonnet-4-20250514' },
      { label: 'OpenRouter', description: 'https://openrouter.ai/api/v1 — many models', url: 'https://openrouter.ai/api/v1', model: 'anthropic/claude-sonnet-4' },
      { label: 'Custom / Local', description: 'Ollama, vLLM, LM Studio or any OpenAI-compatible URL', url: '', model: '' },
    ];
    const pick = await vscode.window.showQuickPick(presets, { placeHolder: 'Which provider is the key for?' });
    if (!pick) return;
    let url = pick.url;
    if (!url) {
      url = (await vscode.window.showInputBox({
        prompt: 'Base URL (OpenAI-compatible, includes /v1)',
        placeHolder: 'http://localhost:11434/v1',
        ignoreFocusOut: true,
      })) ?? '';
      if (!url) return;
    }
    const key = await vscode.window.showInputBox({
      prompt: `API key for ${new URL(url).host} (stored in VS Code SecretStorage — never written to settings)`,
      password: true,
      ignoreFocusOut: true,
    });
    if (!key) return;
    const model = (await vscode.window.showInputBox({
      prompt: 'Model ID to use',
      value: pick.model,
      placeHolder: 'gpt-4o, llama3.1, qwen2.5-coder:14b…',
      ignoreFocusOut: true,
    }));
    if (model === undefined) return;

    let host: string;
    try {
      host = new URL(url).host;
    } catch {
      void vscode.window.showErrorMessage('BitVanes: invalid base URL.');
      return;
    }
    await llm.setApiKey(host, key.trim());
    const cfg = vscode.workspace.getConfiguration('bitvanes.llm');
    await cfg.update('provider', 'openai-compatible', vscode.ConfigurationTarget.Global);
    await cfg.update('baseUrl', url.replace(/\/+$/, ''), vscode.ConfigurationTarget.Global);
    if (model) await cfg.update('modelId', model, vscode.ConfigurationTarget.Global);
    void vscode.window.showInformationMessage(`BitVanes: key stored for ${host}. Provider set to openai-compatible.`);
  });

  register('bitvanes.clearApiKey', async () => {
    const stored = llm.listStored();
    if (stored.length === 0) {
      void vscode.window.showInformationMessage('BitVanes: no stored API keys.');
      return;
    }
    const pick = await vscode.window.showQuickPick(stored, { placeHolder: 'Delete which stored key?' });
    if (!pick) return;
    await llm.deleteApiKey(pick);
    void vscode.window.showInformationMessage(`BitVanes: deleted stored key for ${pick}.`);
  });

  const browseSteps = async () => {
    const st = director?.state;
    if (!st) {
      void vscode.window.showInformationMessage('BitVanes: no walkthrough running. Start one from the BitVanes view or Source Control.');
      return;
    }
    const items = st.plan.steps.map((step, i) => ({
      label: `$(${i === st.current ? 'play' : 'circle-small-filled'}) ${i + 1}. ${step.title}`,
      description: toDisplayPath(step.filePath),
      detail: [
        step.variable
          ? `${step.variable.name} (${step.variable.action})${
              step.variable.stateBefore !== undefined || step.variable.stateAfter !== undefined
                ? `: ${step.variable.stateBefore ?? '?'} → ${step.variable.stateAfter ?? '?'}`
                : ''
            }`
          : undefined,
        step.securityNote ? `⚠ ${step.securityNote}` : undefined,
        step.explanation,
      ]
        .filter(Boolean)
        .join('\n'),
      index: i,
    }));
    const picked = await vscode.window.showQuickPick(items, {
      placeHolder: `Walkthrough steps — ${st.plan.summary}`,
      matchOnDetail: true,
      ignoreFocusOut: true,
    });
    if (picked) director?.jump(picked.index);
  };

  context.subscriptions.push(director, statusbar, { dispose: stopAutoplay });
}

export function deactivate(): void {
  director?.exit();
}
