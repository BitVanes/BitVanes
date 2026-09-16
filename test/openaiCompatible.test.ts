import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { OpenAiCompatibleClient } from '../src/llm/openaiCompatible';

interface CapturedRequest {
  path: string | undefined;
  authorization?: string;
  body: Record<string, unknown>;
}

let server: Server;
let baseUrl = '';
let captured: CapturedRequest | null = null;
let respondWith: (req: CapturedRequest) => { status: number; json: unknown } = () => ({
  status: 200,
  json: { choices: [{ message: { content: '{"steps":[]}' } }] },
});

beforeAll(async () => {
  server = createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => {
      raw += c;
    });
    req.on('end', () => {
      captured = {
        path: req.url,
        authorization: req.headers['authorization'] as string | undefined,
        body: JSON.parse(raw || '{}') as Record<string, unknown>,
      };
      const out = respondWith(captured);
      res.writeHead(out.status, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify(out.json));
    });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const addr = server.address() as AddressInfo;
  baseUrl = `http://127.0.0.1:${addr.port}/v1`;
});

afterAll(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()));
});

function client(opts?: Partial<ConstructorParameters<typeof OpenAiCompatibleClient>[0]>) {
  return new OpenAiCompatibleClient({
    baseUrl,
    modelId: 'test-model',
    getApiKey: async () => 'sk-test-key',
    ...opts,
  });
}

describe('OpenAiCompatibleClient (wire format)', () => {
  it('sends a spec-conformant chat completion request', async () => {
    const out = await client().complete({ system: 'SYS', user: 'USR', maxTokens: 128, temperature: 0.2 });
    expect(out).toBe('{"steps":[]}');
    expect(captured?.path).toBe('/v1/chat/completions');
    expect(captured?.authorization).toBe('Bearer sk-test-key');
    expect(captured?.body['model']).toBe('test-model');
    expect(captured?.body['stream']).toBe(false);
    expect(captured?.body['max_tokens']).toBe(128);
    expect(captured?.body['temperature']).toBe(0.2);
    const messages = captured?.body['messages'] as Array<{ role: string; content: string }>;
    expect(messages).toEqual([
      { role: 'system', content: 'SYS' },
      { role: 'user', content: 'USR' },
    ]);
  });

  it('joins array-shaped content responses', async () => {
    respondWith = () => ({ status: 200, json: { choices: [{ message: { content: [{ text: 'a' }, { text: 'b' }] } }] } });
    const out = await client().complete({ system: 'S', user: 'U', maxTokens: 8, temperature: 0 });
    expect(out).toBe('ab');
    respondWith = () => ({ status: 200, json: { choices: [{ message: { content: '{"steps":[]}' } }] } });
  });

  it('throws with status and body snippet on HTTP errors', async () => {
    respondWith = () => ({ status: 401, json: { error: { message: 'bad key' } } });
    await expect(
      client().complete({ system: 'S', user: 'U', maxTokens: 8, temperature: 0 }),
    ).rejects.toThrow(/HTTP 401.*bad key/);
    respondWith = () => ({ status: 200, json: { choices: [{ message: { content: '{"steps":[]}' } }] } });
  });

  it('omits Authorization when no key is stored', async () => {
    await client({ getApiKey: async () => undefined }).complete({ system: 'S', user: 'U', maxTokens: 8, temperature: 0 });
    expect(captured?.authorization).toBeUndefined();
  });
});
