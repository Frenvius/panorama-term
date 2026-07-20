# Changelog

## [0.2.0]

### Added
- Multi-agent support in the agent bar: Antigravity, Codex and OpenCode are detected alongside Claude, each with its own logo and slash commands.
- Tile palette opened with a double Shift tap.
- Link terminal tiles together so agents can message each other.
- Silent project run and build from terminal tiles, with dotnet build output opening in its own tile.
- Git history tab with a commit graph, unpushed commits and HEAD highlighted, auto-refreshing every 5s.
- Notes linked to terminals as agent-editable markdown files, with a live-preview editor and a context menu.
- Reopen closed tiles with Ctrl+Shift+T, resuming the agent session.
- Resume panel showing a preview of the previous session chat.
- Restart a terminal elevated via gsudo.
- New terminals spawn at the cursor position.
- Zoom on tile focus with Alt+F, and middle-click-only panning.
- Fullscreen button and shortcut.
- Agent activity indicator and session line-diff badge on navigator tiles and frames.
- Navigator state persists across reloads: open state, active tab, collapsed frames, git tab selection.

### Fixed
- Agent identity no longer flips between Claude, Codex and Antigravity when those words appear in terminal output.
- Moving a tile to another tab works instead of crashing.
- Terminal typing restored on Windows and macOS; dead keys and character composition fixed on Linux.
- Ctrl+Backspace sends ctrl+h so raw shells delete a word.
- Wrapped multi-line terminal URLs are fully clickable.
- Wide and symbol glyphs align to the grid instead of drifting.
- Terminal focus routes to the grid on non-Linux.
- Canvas panning works over a focused tile; zoom bounce is faster and no longer blocks panning.
- Notification window stays above a maximized main window and recovers when stuck hidden.
- Duplicate idle notification right after a stop notification suppressed.
- Old notifications no longer replayed when the sidecar reconnects.
- Terminal tiles show their own context menu instead of the native browser one.
- Copying in note render mode strips hidden markdown marks.

### Performance
- Terminal renders as a DOM grid instead of a canvas.
- Git commands run async, removing main-thread UI freezes.
- Idle terminal allocation churn and frame rate cut.
- Sidecar split into a supervised host process, so PTYs survive a restart of the agent brain.

## [0.1.0]

### Added
- Initial release: infinite canvas with terminal tiles, notes, frames, and workspaces.
- Rust sidecar PTY host with Canvas 2D grid terminal rendering.
- Claude agent bar with image paste and structured transcript state.
- Git integration: branch menu with pull, push and branch actions, changes tab with commit, diff viewer with hunk revert, ahead/behind indicators.
- Desktop notifications for agent activity.
- Auto-updater with in-app download and restart.
