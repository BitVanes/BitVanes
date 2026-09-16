# BitVanes

**A guided tour through code you didn't write — diffs, AI-generated changes, or any function you want to understand.**

BitVanes parses your pending changes (or any selected function) into syntax trees, asks a language model to trace the execution and data flow, and plays the result back as synchronized steps in your editor: the active statement spotlighted, everything else dimmed, and variable transitions explained as you step.

Two audiences, one tool: seasoned engineers use it to burn through review diffs quickly; vibe coders use it to finally understand the code their AI just wrote. Pick your depth with **BitVanes: Set Walkthrough Style** — `Expert` (terse, risk-focused), `Standard`, or `Learner` (plain language, syntax explained, jargon defined).

Built for reviewing exactly the kind of code where being wrong is expensive — smart-contract engines, consensus paths, auth flows, migrations — but useful for any change you want to *understand*, not just *read*.

## How it works

1. **Ingest** — `bitvanes.walkthroughDiff` reads your unstaged or staged changes via `git diff --unified=0`; `bitvanes.walkthroughSelection` takes the function under your cursor.
2. **Parse** — tree-sitter grammars (Rust, Go, Solidity, TypeScript/TSX, Python) extract declarations, assignments, and calls around every hunk. Unknown languages fall back to line-based heuristics — nothing crashes.
3. **Generate** — an LLM produces a strict `WalkthroughPlan` JSON: ordered steps, exact 1-based ranges, variable transitions (`init → validate → mutate → commit → return`), and security notes where they matter.
4. **Repair** — ranges are snapped back onto real AST nodes, so slightly-off model output still lands on the exact statement.
5. **Play** — the editor spotlights the active token (amber border, subtle fill), dims everything outside the enclosing scope, and draws a dotted border around the parent function. Hover the highlight for the narrative. Step with `]` / `[`, the status bar controls, or the sidebar step list.

## Install

- **Marketplace:** search "BitVanes" (coming soon)
- **From source:** `npm ci && npm run compile`, then open this folder in VS Code and press F5
- **Prebuilt vsix:** grab the latest from [Releases](https://github.com/BitVanes/BitVanes/releases) and run `code --install-extension bitvanes-<version>.vsix`

## Choosing a model

| Provider | Setup |
|---|---|
| **VS Code Copilot** (default) | None. Sign in to GitHub Copilot; BitVanes uses the built-in Language Model API. No keys, no network config. |
| **Local — Ollama / LM Studio / vLLM** | `ollama serve`, then set `bitvanes.llm.baseUrl` to `http://localhost:11434/v1` and `bitvanes.llm.modelId` to e.g. `qwen2.5-coder:14b`. Code never leaves your machine. |
| **OpenAI / Anthropic / OpenRouter** | Run `BitVanes: Set API Key for External LLM Provider`, pick a preset, paste the key. Keys live in VS Code SecretStorage — never in settings files or dotfiles. |

`bitvanes.llm.provider: "auto"` uses Copilot when available and falls back to the configured endpoint otherwise.

## Commands

| Command | What it does |
|---|---|
| `BitVanes: Instant Walkthrough of Changes (No AI)` | Sub-second local walkthrough from AST analysis — no model, works offline |
| `BitVanes: Walkthrough Working Tree Changes` | AI walkthrough of the unstaged diff |
| `BitVanes: Walkthrough Staged Changes` | AI walkthrough of the staged diff (also in the Source Control title menu) |
| `BitVanes: Walkthrough Code Path at Cursor` | Trace the function under the cursor — no git needed |
| `BitVanes: Next / Previous Step` | Transport — `alt+]` / `alt+[` anywhere; bare `]` / `[` when not typing in the editor |
| `BitVanes: Browse Steps & Explanations` | QuickPick browser with full narratives, state transitions, and security notes |
| `BitVanes: Toggle Autoplay` | Auto-advance through the plan (manual navigation pauses it) |
| `BitVanes: Set Walkthrough Style` | Audience tuning: Expert / Standard / Learner |

## Security model

- API keys are stored via `vscode.SecretStorage` (OS keychain-backed). There is no setting that accepts a key.
- The extension makes exactly two kinds of network calls: the language-model endpoint you configure, and local `git` invocations. Nothing else phones home.
- Grammar binaries are vendored into the repo with pinned SHA-256 checksums (`scripts/fetch-grammars.mjs --check` verifies them in CI).

## Development

```bash
npm ci
node scripts/fetch-grammars.mjs   # vendor grammars (checksum-pinned)
npm run typecheck && npm run lint
npm test                          # 50 tests, incl. real tree-sitter WASM parses
npm run compile                   # esbuild production bundle
npm run package                   # vsce package
```

Press F5 in VS Code to launch the Extension Development Host.

Docs and demo: https://bitvanes.github.io/BitVanes/

## License

Proprietary — see [LICENSE.md](LICENSE.md).
