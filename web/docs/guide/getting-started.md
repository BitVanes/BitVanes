# Getting Started

## Install

::: code-group

```text [Marketplace]
Search "BitVanes" in the VS Code Extensions view (coming soon).
```

```bash [From release]
code --install-extension bitvanes-<version>.vsix
```

```bash [From source]
git clone https://github.com/BitVanes/BitVanes.git
cd BitVanes && npm ci && npm run compile
# open in VS Code, press F5
```

:::

## Your first walkthrough

1. Make a change in a git repo (or just open a function you want to understand).
2. Run **BitVanes: Walkthrough Working Tree Changes** from the Command Palette — or **BitVanes: Walkthrough Code Path at Cursor** with your cursor inside a function.
3. The editor pans to the first step: the active statement is spotlighted, everything outside the enclosing function is dimmed, and a dotted border marks the scope.
4. Step with `]` (next) and `[` (previous), or use the status-bar transport. **Hover the highlighted code** for the full narrative, variable transitions, and any security note.

## Choosing a model

### GitHub Copilot (default, zero config)

If you're signed in to Copilot, BitVanes uses VS Code's built-in Language Model API. Nothing to configure, no keys.

### Local models — Ollama, LM Studio, vLLM

::: code-group

```bash [Ollama]
ollama pull qwen2.5-coder:14b
ollama serve
```

```text [LM Studio]
Start the local server (default http://localhost:1234/v1)
and load a chat model.
```

:::

Then set:

```jsonc
{
  "bitvanes.llm.provider": "openai-compatible",
  "bitvanes.llm.baseUrl": "http://localhost:11434/v1", // Ollama; LM Studio: http://localhost:1234/v1
  "bitvanes.llm.modelId": "qwen2.5-coder:14b"
}
```

Your code never leaves the machine.

### Hosted — OpenAI, Anthropic, OpenRouter

Run **BitVanes: Set API Key for External LLM Provider**, pick a preset, paste your key. The key is stored in VS Code SecretStorage (OS keychain) — there is deliberately no setting that accepts a key.

## Keyboard shortcuts

| Key | Action |
|---|---|
| `alt+]` / `alt+[` | Next / previous step (works anywhere while a walkthrough is active) |
| `]` / `[` | Next / previous step when focus is outside the editor (panel, status bar) |
| `Esc` | Exit walkthrough (from the editor) |
