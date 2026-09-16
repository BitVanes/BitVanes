# Settings

All settings live under `bitvanes.*` in `settings.json`.

## Model

| Setting | Default | Description |
|---|---|---|
| `bitvanes.llm.provider` | `auto` | `auto` (Copilot first, then the configured endpoint), `vscode-lm`, or `openai-compatible` |
| `bitvanes.llm.baseUrl` | `http://localhost:11434/v1` | OpenAI-compatible base URL. Ollama `:11434/v1`, LM Studio `:1234/v1`, vLLM `:8000/v1`, OpenAI `https://api.openai.com/v1`, Anthropic `https://api.anthropic.com/v1`, OpenRouter `https://openrouter.ai/api/v1` |
| `bitvanes.llm.modelId` | *(empty)* | Model ID for the endpoint, e.g. `qwen2.5-coder:14b`, `gpt-4o`, `claude-sonnet-4-20250514`. Ignored for Copilot |
| `bitvanes.llm.temperature` | `0.2` | Low keeps schema-accurate JSON |
| `bitvanes.llm.maxTokens` | `4096` | Max tokens requested from the model |
| `bitvanes.llm.maxSteps` | `12` | Cap on walkthrough length (1–50) |
| `bitvanes.llm.contextLines` | `6` | Context lines padded around each diff hunk in the prompt |

## Playback

| Setting | Default | Description |
|---|---|---|
| `bitvanes.autoplay.intervalMs` | `4000` | Delay between autoplay steps (min 1000); manual navigation pauses autoplay |

## Editor

| Setting | Default | Description |
|---|---|---|
| `bitvanes.editor.spotlightColor` | `#f59e0b` | Spotlight border color (hex) |
| `bitvanes.editor.dimOpacity` | `0.3` | Opacity for code outside the active scope |

::: tip
API keys are deliberately not settings — run **BitVanes: Set API Key for External LLM Provider** instead, which stores them in VS Code SecretStorage.
:::
