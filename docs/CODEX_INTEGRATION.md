# Codex integration in terminal tiles

## Goal

Give Codex tiles the same useful Panorama surfaces as Claude tiles: prompt bar,
session identity, live status, notifications, model and effort controls, context
usage, linked context, and resume. Keep the Codex TUI as the interactive client.

## Sources

- Codex lifecycle hooks: https://learn.chatgpt.com/docs/hooks
- Codex CLI commands: https://learn.chatgpt.com/docs/developer-commands
- Codex plugin packaging: https://developers.openai.com/plugins/build/plugins

## Current state

- The AgentBar already sends prompts to Codex and history can resume Codex sessions.
- Panorama's Windows terminal launches interactive Codex through a tile-scoped
  wrapper. It adds hooks with CLI overrides and `--no-daemon`, without changing
  the user's global Codex configuration. Codex asks for a one-time hook review.
- Session start, prompt submission, tool use, stop, interrupt and session end
  send model, permission mode, and lifecycle status through `/agent-status`.
- Codex `PermissionRequest` alone does not prove a prompt is still waiting: an
  automatic decision may resolve it. Panorama does not raise an attention
  notification until a reliable pending signal exists. The TUI still handles
  interactive approval prompts.
- Prompt submission receives linked Panorama context through `/agent-hook` as
  Codex `UserPromptSubmit.additionalContext`.
- The AgentBar has Codex slash suggestions and uses Codex hook state for identity.
- The AgentBar reads the Codex TUI footer for its active model, effort, Plan
  mode (Shift+Tab), and context remaining when that field is configured.
- Real TUI tests passed with two concurrent tiles: each prompt, session ID,
  response, busy/idle state, and stop stayed with its own tile. Resuming a
  session retained its ID. A linked note title reached the model through the
  prompt hook. The Codex footer exposed model, effort, and measured context.
- Global hooks are intentionally absent. The shared Codex daemon reused a
  different tile's environment in testing; `--no-daemon` isolates it.

## Next slices

1. Verify an actual interactive permission prompt and tool lifecycle in the
   Codex TUI. A `PermissionRequest` hook alone can fire for an automatic review,
   so Panorama does not raise an attention notification from it.
2. Read the current model catalog and session usage from a documented Codex
   interface. Reuse the harness model catalog where its data applies. Show only
   measured context and supported effort levels.
3. Add Codex model and effort controls in AgentBar using Codex's interactive
   commands or app-server requests, based on what changes the active TUI session.
4. Extend mobile approval and chat flows, which currently accept Claude tiles.
5. Replace remaining Claude-only notification labels/icons for Codex events.

## Limits

Codex hooks do not document the Claude mod's prompt-render and suggestion events.
Those UI details need a separate source or a Codex client controlled through
app-server. Do not guess them from the terminal grid as authoritative state.
The automatic wrapper is currently Windows-only.
