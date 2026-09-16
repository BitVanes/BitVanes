---
layout: home

hero:
  name: BitVanes
  text: A guided tour through code you didn't write.
  tagline: BitVanes turns diffs, AI-generated changes, or any function you don't understand into a step-by-step tour inside VS Code — spotlighted statements, traced data flow, and explanations tuned to how deep you want to go.
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
  - icon: 🧭
    title: Three depths, one tour
    details: Expert for seasoned devs burning through review diffs. Standard for working developers. Learner for vibe coders reviewing AI-generated code — same walkthrough, explanations tuned per audience.
  - icon: 🌊
    title: Data-flow tracing
    details: Tours follow execution order — input, validation, state transition, persistence, return — with before → after variable transitions on every step.
  - icon: 🔒
    title: Security-first notes
    details: Auth checks, reentrancy risk, unchecked calls, overflow surface, and trust boundaries get high-contrast callouts while you step.
  - icon: 🤖
    title: Bring your own model
    details: GitHub Copilot via the VS Code Language Model API out of the box, or point it at Ollama, vLLM, LM Studio, OpenAI, Anthropic, or OpenRouter. Keys stay in SecretStorage.
  - icon: ⚡
    title: Instant, offline tours
    details: Need speed or no model at all? Instant mode builds a local AST walkthrough in under a second — and unsupported languages fall back to line-based heuristics instead of crashing.
---
