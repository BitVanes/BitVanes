import type { CancellationToken } from 'vscode';

export interface LlmCompleteRequest {
  system: string;
  user: string;
  maxTokens: number;
  temperature: number;
}

export interface OpenAiCompatibleOptions {
  label?: string;
  baseUrl: string;
  modelId: string;
  getApiKey: () => Promise<string | undefined>;
}

export class OpenAiCompatibleClient {
  readonly id = 'openai-compatible';
  readonly label: string;

  constructor(private readonly opts: OpenAiCompatibleOptions) {
    this.label = this.opts.label ?? `${this.opts.modelId} @ ${hostOf(this.opts.baseUrl)}`;
  }

  async available(): Promise<boolean> {
    return this.opts.modelId.trim() !== '' && this.opts.baseUrl.trim() !== '';
  }

  async complete(req: LlmCompleteRequest, token?: CancellationToken): Promise<string> {
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
      let res: Response;
      try {
        res = await fetch(url, {
          method: 'POST',
          headers,
          signal: AbortSignal.any([abort.signal, AbortSignal.timeout(REQUEST_TIMEOUT_MS)]),
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
      } catch (err) {
        if (err instanceof Error && (err.name === 'TimeoutError' || err.name === 'AbortError') && !token?.isCancellationRequested) {
          throw new Error(`request to ${hostOf(this.opts.baseUrl)} timed out after ${REQUEST_TIMEOUT_MS / 1000}s`);
        }
        if (err instanceof TypeError) {
          throw new Error(
            `could not reach ${hostOf(this.opts.baseUrl)} — is the model server running? (e.g. \`ollama serve\`, then retry)`,
          );
        }
        throw err;
      }
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

const REQUEST_TIMEOUT_MS = 120_000;

export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

const LOCAL_ENDPOINTS = [
  { url: 'http://localhost:11434/v1', label: 'Ollama' },
  { url: 'http://localhost:1234/v1', label: 'LM Studio' },
  { url: 'http://localhost:8000/v1', label: 'vLLM' },
];

export async function detectLocalModelServer(): Promise<{ url: string; label: string; modelId: string } | null> {
  for (const ep of LOCAL_ENDPOINTS) {
    try {
      const res = await fetch(`${ep.url}/models`, { signal: AbortSignal.timeout(1_500) });
      if (!res.ok) continue;
      const json = (await res.json()) as { data?: Array<{ id?: string }> };
      const modelId = json.data?.find((m) => m.id)?.id;
      if (modelId) return { url: ep.url, label: ep.label, modelId };
    } catch {
      continue;
    }
  }
  return null;
}
