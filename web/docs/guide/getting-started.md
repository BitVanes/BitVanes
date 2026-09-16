# Getting Started

BitVanes is a guided tour through code you didn't write — a pending diff, a fresh AI-generated change, or any function you want to finally understand. It plays the tour back inside your editor: the active statement spotlighted, everything else dimmed, data flow explained step by step.

**Two audiences, one tool.** Seasoned engineers use it to burn through review diffs quickly (`Expert` style). Newer developers — and anyone reviewing code an AI generated for them — get plain-language tours that explain the syntax and define the jargon (`Learner` style).

## Choosing your style

Run **BitVanes: Set Walkthrough Style** (also in the panel's ⋯ menu) or set `bitvanes.walkthrough.style`:

| Style | For | What changes |
|---|---|---|
| **Expert** | Senior engineers fluent in the language | Terse, high-signal steps; no syntax lessons; invariants and risk only |
| **Standard** *(default)* | Working developers new to this code | What the code does and why it exists in the flow |
| **Learner** | New devs and vibe coders | Plain language, syntax explained on first use, jargon defined, why the pattern matters |

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

## Your first tour

1. Make a change in a git repo (or just open a function you want to understand).
2. Run **BitVanes: Walkthrough Working Tree Changes** from the Command Palette — or **BitVanes: Walkthrough Code Path at Cursor** with your cursor inside a function. In a hurry? **Instant Walkthrough (No AI)** builds a local tour in under a second.
3. The editor pans to the first step: the active statement is spotlighted, everything outside the enclosing function is dimmed, and a dotted border marks the scope.
4. Step with `alt+]` / `alt+[`, or use the status-bar transport. **Hover the highlighted code** for the full narrative, variable transitions, and any security note.

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
