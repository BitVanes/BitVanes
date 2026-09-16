---
layout: home

hero:
  name: BitVanes
  text: Watch your code think.
  tagline: Turns diffs and code paths into interactive, AI-guided walkthroughs inside VS Code — spotlighted statements, dimmed noise, traced variables, security callouts.
  image:
    src: /logo.svg
    alt: BitVanes
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: View on GitHub
      link: https://github.com/BitVanes/BitVanes

features:
  - icon: 🎯
    title: AST-accurate stepping
    details: tree-sitter grammars for Rust, Go, Solidity, TypeScript, and Python. Every step highlights the exact statement — model output is snapped back onto real syntax nodes.
  - icon: 🌊
    title: Data-flow tracing
    details: Steps follow execution order — input, validation, state transition, persistence, return — with before → after variable transitions on every step.
  - icon: 🔒
    title: Security-first notes
    details: Auth checks, reentrancy risk, unchecked calls, overflow surface, and trust boundaries get high-contrast callouts while you step.
  - icon: 🤖
    title: Bring your own model
    details: GitHub Copilot via the VS Code Language Model API out of the box, or point it at Ollama, vLLM, LM Studio, OpenAI, Anthropic, or OpenRouter. Keys stay in SecretStorage.
  - icon: 📝
    title: Diff-native
    details: Walk your unstaged or staged changes straight from the Source Control menu — or trace any function under the cursor, no git required.
  - icon: 🛡️
    title: Degrades gracefully
    details: Unsupported language or broken parse? BitVanes falls back to line-based heuristics instead of crashing — a walkthrough always comes back.
---
