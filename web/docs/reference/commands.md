# Commands

All commands live under the **BitVanes** category in the Command Palette.

| Command | ID | Description |
|---|---|---|
| Instant Walkthrough of Changes (No AI) | `bitvanes.walkthroughInstant` | Sub-second local walkthrough built from AST analysis — no model, works offline |
| Walkthrough Working Tree Changes | `bitvanes.walkthroughDiff` | Generate a walkthrough of unstaged changes |
| Walkthrough Staged Changes | `bitvanes.walkthroughStagedDiff` | Generate a walkthrough of staged changes (also in the Source Control title menu) |
| Walkthrough Code Path at Cursor | `bitvanes.walkthroughSelection` | Trace the function/block under the cursor or the active selection (also in the editor context menu) |
| Next Step | `bitvanes.nextStep` | Advance; `alt+]` anywhere, bare `]` when not typing in the editor |
| Previous Step | `bitvanes.prevStep` | Go back; `alt+[` anywhere, bare `[` when not typing in the editor |
| Jump to Step | `bitvanes.jumpToStep` | Used internally by the sidebar step list |
| Browse Steps & Explanations | `bitvanes.explainStep` | QuickPick browser over all steps with full narratives |
| Toggle Autoplay | `bitvanes.toggleAutoplay` | Start/stop auto-advancing playback |
| Exit Walkthrough | `bitvanes.exitWalkthrough` | Clear decorations and end the session (`Esc` from the editor) |
| Set API Key for External LLM Provider | `bitvanes.setApiKey` | Store a provider key in SecretStorage and point the config at it |
| Delete Stored API Key | `bitvanes.clearApiKey` | Remove a stored key |
