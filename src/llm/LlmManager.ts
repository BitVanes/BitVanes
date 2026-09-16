import * as vscode from 'vscode';
import { PlanValidationError, validateWalkthroughPlan, type WalkthroughPlan } from '../types/protocol';
import { extractJson } from './json';
import { buildRepairPrompt, buildUserPrompt, buildSystemPrompt, type WalkthroughRequest, type WalkthroughStyle } from './prompts';
import { OpenAiCompatibleClient, detectLocalModelServer, hostOf, type LlmCompleteRequest } from './openaiCompatible';

export { extractJson } from './json';
export { OpenAiCompatibleClient, detectLocalModelServer } from './openaiCompatible';
export type { LlmCompleteRequest } from './openaiCompatible';

export interface LlmClient {
  readonly id: string;
  readonly label: string;
  available(): Promise<boolean>;
  complete(req: LlmCompleteRequest, token?: vscode.CancellationToken): Promise<string>;
}

interface VscodeLmApi {
  selectChatModels(selector?: vscode.LanguageModelChatSelector): Thenable<vscode.LanguageModelChat[]>;
}

const MODEL_PREFERENCE: RegExp[] = [
  /gpt-5/i,
  /gpt-4\.1|gpt-4o/i,
  /claude.*sonnet/i,
  /claude/i,
  /gemini-2\.5/i,
  /gemini-2/i,
  /o[34]/i,
];

function lmApi(): VscodeLmApi | undefined {
  return (vscode as unknown as { lm?: VscodeLmApi }).lm;
}

export class VscodeLmClient implements LlmClient {
  readonly id = 'vscode-lm';
  private modelsProbe: Promise<vscode.LanguageModelChat[]> | null = null;
  private preferred: vscode.LanguageModelChat | null = null;
  private readonly failed = new Set<string>();

  get label(): string {
    if (this.preferred) return `VS Code Copilot (${this.preferred.name || this.preferred.id})`;
    return 'VS Code Copilot (Language Model API)';
  }

  async available(): Promise<boolean> {
    return (await this.listModels()).length > 0;
  }

  private listModels(): Promise<vscode.LanguageModelChat[]> {
    if (!this.modelsProbe) {
      const probe = (async () => {
        const lm = lmApi();
        if (!lm) return [];
        try {
          const models = await lm.selectChatModels({});
          if (!Array.isArray(models)) return [];
          const seen = new Set<string>();
          return models.filter((m) => {
            if (seen.has(m.id)) return false;
            seen.add(m.id);
            return true;
          });
        } catch {
          return [];
        }
      })();
      this.modelsProbe = probe;
      // A cold-start probe (e.g. Copilot still signing in) must not poison the
      // whole session — only successful, non-empty results stay cached.
      void probe.then(
        (models) => {
          if (models.length === 0) this.modelsProbe = null;
        },
        () => {
          this.modelsProbe = null;
        },
      );
    }
    return this.modelsProbe;
  }

  private rank(models: vscode.LanguageModelChat[]): vscode.LanguageModelChat[] {
    return [...models].sort((a, b) => {
      if (a.id === this.preferred?.id) return -1;
      if (b.id === this.preferred?.id) return 1;
      return preferenceScore(b) - preferenceScore(a);
    });
  }

  async complete(req: LlmCompleteRequest, token?: vscode.CancellationToken): Promise<string> {
    let models = (await this.listModels()).filter((m) => !this.failed.has(m.id));
    if (models.length === 0) {
      // Every model already failed this session (e.g. a transient sign-in
      // hiccup blacklisted them all) — give them one fresh chance.
      this.failed.clear();
      models = await this.listModels();
    }
    const ordered = this.rank(models);
    if (ordered.length === 0) throw new Error('no usable VS Code language models');

    const errors: string[] = [];
    for (const model of ordered.slice(0, 12)) {
      if (token?.isCancellationRequested) throw new Error('request cancelled');
      try {
        const out = await this.runOnce(model, req, token);
        this.preferred = model;
        return out;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        this.failed.add(model.id);
        errors.push(`${model.name || model.id}: ${msg}`);
      }
    }
    throw new Error(`every Copilot model rejected the request — ${errors.join(' | ')}`);
  }

  private async runOnce(
    model: vscode.LanguageModelChat,
    req: LlmCompleteRequest,
    token?: vscode.CancellationToken,
  ): Promise<string> {
    const messages = [
      vscode.LanguageModelChatMessage.Assistant(req.system),
      vscode.LanguageModelChatMessage.User(req.user),
    ];
    const response = await model.sendRequest(messages, {}, token);
    let out = '';
    for await (const chunk of response.text) {
      out += chunk;
      if (out.length > 1_000_000) break;
    }
    if (out.trim() === '') throw new Error('empty response');
    return out;
  }
}

function preferenceScore(model: vscode.LanguageModelChat): number {
  const s = `${model.name} ${model.family} ${model.id}`;
  for (let i = 0; i < MODEL_PREFERENCE.length; i++) {
    if (MODEL_PREFERENCE[i]!.test(s)) return MODEL_PREFERENCE.length - i;
  }
  return 0;
}

export class LlmManager {
  private lmClient: VscodeLmClient | null = null;

  constructor(private readonly secrets: vscode.SecretStorage, private readonly globalState: vscode.Memento) {}

  async buildChain(): Promise<LlmClient[]> {
    const cfg = vscode.workspace.getConfiguration('bitvanes.llm');
    const provider = cfg.get<string>('provider', 'auto');
    const chain: LlmClient[] = [];

    if (provider === 'auto' || provider === 'vscode-lm') {
      if (!this.lmClient) this.lmClient = new VscodeLmClient();
      if (await this.lmClient.available()) chain.push(this.lmClient);
      if (provider === 'vscode-lm' && chain.length === 0) {
        throw new Error(
          'No VS Code language models available. Sign in to GitHub Copilot, or switch bitvanes.llm.provider to "openai-compatible".',
        );
      }
    }
    if (provider === 'auto' || provider === 'openai-compatible') {
      const baseUrl = cfg.get<string>('baseUrl', '');
      const modelId = cfg.get<string>('modelId', '');
      if (baseUrl && modelId) {
        chain.push(
          new OpenAiCompatibleClient({
            baseUrl,
            modelId,
            getApiKey: () => this.getApiKey(hostOf(baseUrl)),
          }),
        );
      } else if (provider === 'openai-compatible') {
        throw new Error(
          'Set bitvanes.llm.baseUrl and bitvanes.llm.modelId (e.g. Ollama: http://localhost:11434/v1 + qwen2.5-coder:14b), or run "BitVanes: Set API Key".',
        );
      } else {
        const local = await detectLocalModelServer();
        if (local) {
          chain.push(
            new OpenAiCompatibleClient({
              label: `${local.label} (auto-detected: ${local.modelId})`,
              baseUrl: local.url,
              modelId: local.modelId,
              getApiKey: () => Promise.resolve(undefined),
            }),
          );
        }
      }
    }

    if (chain.length === 0) {
      throw new Error(
        'No language model configured. Options: (1) sign in to GitHub Copilot, (2) start Ollama/LM Studio/vLLM locally, or (3) run "BitVanes: Set API Key" for a hosted provider.',
      );
    }
    return chain;
  }

  async generatePlan(
    req: WalkthroughRequest,
    opts: { maxSteps: number; maxTokens: number; temperature: number; style?: WalkthroughStyle },
    token?: vscode.CancellationToken,
  ): Promise<{ plan: WalkthroughPlan; provider: string }> {
    const chain = await this.buildChain();
    const failures: string[] = [];
    let sawModelNotSupported = false;
    const system = buildSystemPrompt(opts.style ?? 'standard');

    for (const client of chain) {
      try {
        const user = buildUserPrompt(req, opts.maxSteps);
        let raw = await client.complete({ system, user, ...opts }, token);
        let plan: WalkthroughPlan;
        const defaults = { maxSteps: opts.maxSteps, defaultFilePath: req.mode === 'selection' ? req.selection?.path : undefined };
        try {
          plan = validateWalkthroughPlan(extractJson(raw), defaults);
        } catch (err) {
          if (err instanceof PlanValidationError) {
            raw = await client.complete(
              { system, user: buildRepairPrompt(raw, err.issues.map((i) => `${i.field}: ${i.message}`).join('; ')).slice(0, 2_000), ...opts },
              token,
            );
            plan = validateWalkthroughPlan(extractJson(raw), defaults);
          } else {
            throw err;
          }
        }
        return { plan, provider: client.label };
      } catch (err) {
        if (token?.isCancellationRequested) throw new Error('Walkthrough generation cancelled');
        failures.push(`${client.label}: ${err instanceof Error ? err.message : String(err)}`);
        if (/model_not_supported/.test(err instanceof Error ? err.message : String(err))) {
          sawModelNotSupported = true;
        }
      }
    }
    let message = `All model providers failed.\n${failures.join('\n')}`;
    if (sawModelNotSupported) {
      message +=
        '\n\nCopilot rejected every model. Most common cause: the Copilot Language Model API only serves Marketplace-published extensions and Extension Development Host sessions — a VSIX-installed extension is not allow-listed. Either: (1) launch a dev window with `code --extensionDevelopmentPath=<this extension folder>`, (2) start a local model (e.g. `ollama serve` — BitVanes auto-detects it), or (3) run "BitVanes: Set API Key" for a hosted provider.';
    }
    throw new Error(message);
  }

  async setApiKey(origin: string, key: string): Promise<void> {
    await this.secrets.store(this.secretKey(origin), key);
    const idx = this.listStored();
    if (!idx.includes(origin)) {
      await this.globalState.update('bitvanes.storedApiKeys', [...idx, origin]);
    }
  }

  async getApiKey(origin: string): Promise<string | undefined> {
    try {
      return await this.secrets.get(this.secretKey(origin));
    } catch {
      return undefined;
    }
  }

  async deleteApiKey(origin: string): Promise<void> {
    await this.secrets.delete(this.secretKey(origin));
    await this.globalState.update(
      'bitvanes.storedApiKeys',
      this.listStored().filter((o) => o !== origin),
    );
  }

  listStored(): string[] {
    return this.globalState.get<string[]>('bitvanes.storedApiKeys', []);
  }

  private secretKey(origin: string): string {
    return `bitvanes.apiKey.${origin}`;
  }
}
