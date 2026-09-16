import * as vscode from 'vscode';
import { PlanValidationError, validateWalkthroughPlan, type WalkthroughPlan } from '../types/protocol';
import { extractJson } from './json';
import { buildRepairPrompt, buildUserPrompt, SYSTEM_PROMPT, type WalkthroughRequest } from './prompts';

export { extractJson } from './json';

export interface LlmCompleteRequest {
  system: string;
  user: string;
  maxTokens: number;
  temperature: number;
}

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
      this.modelsProbe = (async () => {
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
    const models = (await this.listModels()).filter((m) => !this.failed.has(m.id));
    const ordered = this.rank(models);
    if (ordered.length === 0) throw new Error('no usable VS Code language models (all candidates failed this session)');

    const errors: string[] = [];
    for (const model of ordered.slice(0, 5)) {
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

export interface OpenAiCompatibleOptions {
  label?: string;
  baseUrl: string;
  modelId: string;
  getApiKey: () => Promise<string | undefined>;
}

export class OpenAiCompatibleClient implements LlmClient {
  readonly id = 'openai-compatible';
  readonly label: string;

  constructor(private readonly opts: OpenAiCompatibleOptions) {
    this.label = this.opts.label ?? `${this.opts.modelId} @ ${hostOf(this.opts.baseUrl)}`;
  }

  async available(): Promise<boolean> {
    return this.opts.modelId.trim() !== '' && this.opts.baseUrl.trim() !== '';
  }

  async complete(req: LlmCompleteRequest, token?: vscode.CancellationToken): Promise<string> {
    const url = `${this.opts.baseUrl.replace(/\/+$/, '')}/chat/completions`;
    const key = await this.opts.getApiKey();
    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (key) headers['Authorization'] = `Bearer ${key}`;
    if (/anthropic/i.test(this.opts.baseUrl)) {
      headers['anthropic-version'] = '2023-06-01';
      if (key) headers['x-api-key'] = key;
    }

    const abort = new AbortController();
    const sub = token?.onCancellationRequested(() => abort.abort());
    try {
      const res = await fetch(url, {
        method: 'POST',
        headers,
        signal: abort.signal,
        body: JSON.stringify({
          model: this.opts.modelId,
          messages: [
            { role: 'system', content: req.system },
            { role: 'user', content: req.user },
          ],
          temperature: req.temperature,
          max_tokens: req.maxTokens,
          stream: false,
        }),
      });
      if (!res.ok) {
        const body = await res.text().catch(() => '');
        throw new Error(`HTTP ${res.status} from ${hostOf(this.opts.baseUrl)}: ${body.slice(0, 300)}`);
      }
      const json = (await res.json()) as {
        choices?: Array<{ message?: { content?: string | Array<{ text?: string }> } }>;
      };
      const content = json.choices?.[0]?.message?.content;
      if (typeof content === 'string') return content;
      if (Array.isArray(content)) return content.map((c) => c.text ?? '').join('');
      throw new Error('response contained no message content');
    } finally {
      sub?.dispose();
    }
  }
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
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
      const client = new OpenAiCompatibleClient({
        baseUrl,
        modelId,
        getApiKey: () => this.getApiKey(hostOf(baseUrl)),
      });
      if (await client.available()) chain.push(client);
    }

    if (chain.length === 0) {
      throw new Error(
        'No language model configured. Use the VS Code Copilot extension, or set bitvanes.llm.modelId and bitvanes.llm.baseUrl (e.g. Ollama at http://localhost:11434/v1).',
      );
    }
    return chain;
  }

  async generatePlan(
    req: WalkthroughRequest,
    opts: { maxSteps: number; maxTokens: number; temperature: number },
    token?: vscode.CancellationToken,
  ): Promise<{ plan: WalkthroughPlan; provider: string }> {
    const chain = await this.buildChain();
    const failures: string[] = [];

    for (const client of chain) {
      try {
        const user = buildUserPrompt(req, opts.maxSteps);
        let raw = await client.complete({ system: SYSTEM_PROMPT, user, ...opts }, token);
        let plan: WalkthroughPlan;
        try {
          plan = validateWalkthroughPlan(extractJson(raw), { maxSteps: opts.maxSteps });
        } catch (err) {
          if (err instanceof PlanValidationError) {
            raw = await client.complete(
              { system: SYSTEM_PROMPT, user: buildRepairPrompt(raw, err.issues.map((i) => `${i.field}: ${i.message}`).join('; ')).slice(0, 2_000), ...opts },
              token,
            );
            plan = validateWalkthroughPlan(extractJson(raw), { maxSteps: opts.maxSteps });
          } else {
            throw err;
          }
        }
        return { plan, provider: client.label };
      } catch (err) {
        if (token?.isCancellationRequested) throw new Error('Walkthrough generation cancelled');
        failures.push(`${client.label}: ${err instanceof Error ? err.message : String(err)}`);
      }
    }
    throw new Error(`All model providers failed.\n${failures.join('\n')}`);
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
