# Walkthroughs

BitVanes walkthroughs are read-only: they highlight and explain, but never edit your files. Two flows are supported.

## Diff walkthroughs

- **BitVanes: Walkthrough Working Tree Changes** — traces `git diff` of unstaged edits.
- **BitVanes: Walkthrough Staged Changes** — traces what's staged (`git diff --cached`). Both are also one click away in the Source Control title menu.

Hunks are parsed with `--unified=0`, padded with context lines, enriched with AST symbols around each change, and handed to the model as line-numbered context — the model returns steps that reference the **new** file content, so ranges always match what you see in the editor.

## Code-path walkthroughs

Put your cursor anywhere inside a function (no selection needed — BitVanes expands to the enclosing block, using the AST when the language is supported) and run **BitVanes: Walkthrough Code Path at Cursor**. This is the flow for questions like *"how does this contract's transfer actually execute?"* — the plan follows data from entry to persistence/return.

## The step UI

Every step gives you:

- **Spotlight** — amber border and subtle fill on the exact statement; hover it for the narrative, `variable: before → after`, and any security callout.
- **Dimming** — code outside the active scope drops to 30% opacity so the relevant lines carry the eye.
- **Scope border** — a dotted outline around the enclosing function or block.
- **Sidebar** — the step list with variable/action badges; click any step to jump.
- **Status bar** — `BitVanes 3/7` with prev/next controls and autoplay.
- **Browser** — `BitVanes: Browse Steps & Explanations` opens a QuickPick with every step's full explanation; search across them with fuzzy match.

## Autoplay

`BitVanes: Toggle Autoplay` advances the plan on an interval (default 6s, configurable via `bitvanes.autoplay.intervalMs`) and stops automatically at the last step — good for hands-free review or recording a demo.

## What the model returns

Every plan conforms to one JSON schema (see `src/types/protocol.ts`). Validation is strict and self-healing: 0-based coordinates get normalized, invalid steps are dropped rather than failing the plan, and ranges that land slightly off a real syntax node are **snapped** to the nearest matching AST node before playback starts.
