# Changelog

## [0.4.3]

### Added
- AI actions settings use the same model picker as orchestration: discovered models per provider, plus any model name you type. Leaving the model empty uses the CLI default.

### Fixed
- Tidy projects now reuses existing project frames instead of creating duplicates, and fills in a missing project path, title or color on frames.
- Tiles already in a frame stay inside it when Tidy rearranges that frame.
- Terminals resend their size once the connection is ready, so they no longer start at the wrong size.
- Settings dropdowns close when clicking outside them.

### Changed
- Release builds are published for Windows and Linux only for now.

## [0.4.2]

### Added
- AI actions: generate commit messages from the Git tab with one click, using a configurable provider and model (Settings > AI actions), with per-action overrides.
- Tidy projects with AI (Ctrl+Alt+T): proposes a cleaner grouping and order for projects and frames, applied after you review it.
- Drag to reorder projects and frames in the projects list.

### Fixed
- Resolving a project's git root now works from remote and mobile sessions.

## [0.4.1]

### Added
- Frame Reorder submenu with Auto, Horizontal, Vertical, Grid 2x2 and Grid 3x3 arrangements. Tiles keep their size and the frame wraps around them.
- Orchestration discovers each agent's models live and accepts custom model names, with a searchable model picker.

### Fixed
- Compact reorder no longer leaves gaps between tiles or inside the frame.
- Orchestration boards stay in sync with live workers.
- Defaults and fixtures no longer contain local workspace paths.

## [0.4.0]

### Added
- Multi-agent orchestration: a coordinator agent splits work into tasks, runs worker agents (Claude, Codex, opencode, kimi, pi) in their own git worktrees, verifies their results and merges them.
- Kanban boards for orchestration runs, with Needs you, Queued, Running, Verifying, Review and Done columns. Add tasks, pick the agent and model, start them yourself, review and accept or reopen with a note.
- Orchestrate toggle in the AgentBar, plus an always-on hint that can suggest or start orchestration on its own (Settings > Orchestration).
- Tasks are routed by role (explore, implement, test, review, mechanical), each with its own model chain, and the board shows the cost per task, role and model.
- Boards belong to projects: right-click a tile, a frame or a project to open its boards or create a new one. A loose tile can be turned into a project first.
- New terminals created inside a project frame start in the project folder.
- Disk-backed scrollback and Ctrl+F search in terminals.
- Terminal tiles and split panels snap to the terminal cell grid.
- Closing a project view tab keeps its terminals running.
- Redesigned settings, with grouped sections and search.

### Fixed
- The brain's local HTTP server rejects requests coming from web pages.
- Run commands report the real exit code of PowerShell commands.
- The terminal reports its real size after the host resizes it.
- The native webview context menu no longer appears.
- Stale Claude hooks left by the old sidecar binary are removed.
- Panel gaps are aligned and dock resize handles are centered.

## [0.3.3]

### Added
- Undo and redo for canvas layout changes: moving, resizing, adding and removing tiles.
- Reorder the canvas into a grid-aligned layout, with a compact mode.
- Project tiles spawn inside their frame, and git worktrees are treated as projects.
- Layout spacing applies to every automatically placed tile.
- In project view, notes tint their body and header, and panels show tabs.
- Codex tiles show up in the mobile app's chat and notifications.
- Naming things now uses inline inputs instead of native prompt dialogs.

### Fixed
- Windows updates no longer fail with "Error opening file for writing" when the sidecar is running.
- The terminal sits flush against the AgentBar, with no empty strip at the bottom.

## [0.3.2]

### Fixed
- Switching tabs no longer copies the previous tab's tiles into the tab you switch to.

## [0.3.1]

### Added
- Remote desktop: control a workspace running on another computer over the LAN, Tailscale or the relay. Actions that only make sense on the host, such as "Reveal in explorer", are disabled while connected remotely.
- Project mode: each tab can switch between Canvas and Project mode. Project mode shows terminals, notes and files as tabbed panels that split right or down, with Files, Source Control, Docker and Project Notes panels docked on the left or right.
- A Projects panel lists your project folders. Frames on the canvas can be linked to a project folder, and new terminals and notes open inside them.
- The macOS app is now signed with a Developer ID and notarized by Apple, so it opens without the "unidentified developer" warning. After this update, macOS asks once more for permissions you already granted.

### Fixed
- The AgentBar no longer covers the last lines of terminal output.
- Mouse selection in the terminal lines up with the drawn cells.
- Agent project names are read correctly from Windows paths, also when viewed from another computer.

## [0.3.0]

### Added
- Plugins: folders with a `plugin.json` in the config folder are discovered automatically and listed in Settings > Plugins, with errors shown and an on/off switch per plugin.
- Plugins can add dark and light themes, including the terminal colors. Settings > Appearance picks one theme per mode, and System switches between them.
- Plugins can add fonts. Settings > Appearance picks the interface, code and terminal font, and the terminal measures the chosen font so columns stay aligned.
- Plugins can add syntax coloring for new languages or map more file extensions to a known one, in the editor, the diff viewer and Markdown code blocks.
- Themes can show a still or animated background behind the canvas and make tiles partly transparent. The background hides while a tile is fullscreen or when the system asks to reduce motion.
- The AgentBar shows a chip with the agent's background shells and their live output.
- The Windows app and installer are now digitally signed, so Windows shows a verified publisher instead of "Unknown publisher".

### Fixed
- The titlebar and status bar stay clickable while a tile is fullscreen.
- Linux (AppImage): color emoji no longer crash the window on systems that only ship COLRv1 emoji fonts, such as Fedora.
- Linux (AppImage): notification sounds no longer crash the window.
- Linux (AppImage): terminals and programs launched from Panorama no longer pick up the AppImage's bundled libraries.

## [0.2.19]

### Fixed
- The app no longer freezes or runs out of memory when the git panel is open on a repository with hundreds of thousands of changed or untracked files. Very large sections now show a count with a "Show all" option.
- Notifications no longer cover the status bar, so it stays clickable.

### Performance
- The git changes list, commit history and commit file list only draw the rows on screen, so huge lists scroll smoothly.
- The git panel waits longer between refreshes on repositories where git status is slow, instead of running it nonstop.
- Expanding a large folder in the file tree no longer freezes the window, and folders with many entries show them in pages of 500.
- Files larger than 2 MB open a "File too large to diff" notice instead of locking up the diff viewer, and very long files skip syntax coloring.
- The branch menu starts with remote folders collapsed when a repository has more than 200 remote branches.
- Opening a long agent conversation on the mobile app reads only its latest part.

## [0.2.18]

### Fixed
- The window no longer goes blank when the git panel shows a repository with a very large number of changed or untracked files.

## [0.2.17]

### Added
- The tile palette is now a command bar (Shift Shift) that runs commands as well as jumping to tiles.
- A fullscreen tile docks beside the menu and can be split side by side with the diff view.
- The prompt bar and tile status show Claude subagents while they run.
- A crash inside one panel no longer takes down the whole window; UI errors are written to a log file.

### Fixed
- Reopened terminals no longer draw new output on top of the old screen, and a leftover agent screen no longer brings up the prompt bar when no agent is running.
- Terminal history keeps up to 5000 lines across restarts instead of only the last moments of agent output, and survives background service restarts intact.
- Notification toasts no longer steal focus.
- Closing the tile palette returns focus to where it was.

## [0.2.16]

### Added
- When an agent works in a git worktree, the tile header and git panel follow the worktree instead of the shell folder.

### Fixed
- The conversation history menu lists conversations from the agent's worktree, so sessions started there show up.
- The conversation history menu no longer lists the conversation already open in the tile.

## [0.2.15]

### Fixed
- Mouse scrolling, clicking and selection in terminal apps such as Claude Code now work on Windows 10. Panorama ships its own console host instead of relying on the older one built into Windows. Terminals open before the update restart once.

## [0.2.14]

### Added
- The maximum zoom setting can go below 100%, down to 80%.

### Fixed
- Tile cards in the navigator show at most one line of summary and keep the same height when there is none.

## [0.2.13]

### Added
- Codex tiles get the prompt bar, live status, notifications, model and effort info, and linked context, like Claude tiles.
- The phone tile list shows each tile's last agent interaction.

### Fixed
- Linked agents can read a peer terminal's scrollback, not only what is visible on its screen.

## [0.2.12]

### Fixed
- Phone notifications for the same tile replace each other instead of stacking up.
- No phone notifications while you are using the desktop.
- The phone reconnects faster after waking up or switching networks.

## [0.2.11]

### Added
- Phone companion: pair a phone with Panorama from Settings > Mobile, then reach your terminals over the local network or, when away, through a relay.
- Answer Claude permission prompts, questions and plan approvals from the phone, including Allow, Always allow and Deny right from the notification.
- Agent tiles show the conversation history in the tile header.
- Navigator tile cards show a snippet of each tile's latest activity.

### Fixed
- Minimap status tiles are drawn at their real size.

## [0.2.10]

### Added
- Branch chip and branch popup update live when the repository changes (checkout, commit, fetch, rebase), with background fetch while the window is focused.
- Toast feedback for branch operations, with Rollback after checkout and Restore after deleting a branch.
- Merge, rebase, cherry-pick and revert in progress show a banner with Continue, Skip and Abort, and the branch chip shows the state.
- Checkout with conflicting local changes offers "Stash & checkout".
- Deleting an unmerged branch lists the commits that would be lost.
- Fuzzy branch search with highlighted matches, a Recent section, keyboard navigation, pull with rebase and checkout-and-rebase.
- "Show diff with working tree" on any branch lists the differing files and opens them in the diff viewer.
- Run button phases come from the actual run status, with confirmed stop and ready detection.

### Fixed
- Branches with slashes in the name (feature/x) are shown in full on the tile chip.
- "Compare with current" always showed an empty list.
- Agents in terminals outside the view are detected again after switching tabs.
- Overlapping agent tiles keep a gap in the minimap.
- Terminals no longer inherit the Claude session environment of the app; run icons are larger.

## [0.2.9]

### Added
- Minimap tiles are colored by agent status.
- Run button shows progress phases, and stopping it kills the running process.

### Fixed
- Switching tabs no longer shows the "Resume session" panel over a Claude session that is still running.
- Sending from the agent bar no longer fails when Claude's input cannot be cleared.

### Performance
- Tab switching is instant: terminals reappear with their last screen instead of blank while they reconnect.
- Opening a notification from another tab lands directly on the tile instead of panning after the switch.

## [0.2.8]

### Added
- Version badge in the status bar with a "Check for updates" menu.
- Run and Build buttons are filled, with an inline stop and status badges.
- Agents can see the build session and its failures in the run context.
- Rewinding a Claude prompt (/rewind or Esc Esc) puts it back in the agent bar.
- The agent bar hides while Claude shows a dialog (permissions, questions, /model) and shows Claude's next-prompt suggestion; Tab accepts it.

### Fixed
- Claude agent bar losing the model, effort and context after /clear, /compact or a resumed session; context now drops to 0% right after a compact or clear.
- Agent bar draft lost when the bar remounted after picking an agent.

## [0.2.7]

### Added
- macOS and Linux builds (macOS is not notarized: right-click > Open on first launch).
- Native macOS titlebar with compact traffic lights.
- Claude Code integration now lives entirely in the Panorama mod; Panorama no longer writes hooks or a statusline into ~/.claude/settings.json and restores your previous statusline.
- Claude model picker shows the 200k / 1M toggle only for models where it applies (Opus 4.6, Sonnet 4.6), lists only the effort levels each model supports, marks its default effort, offers fast mode where available and warns about Fable usage.
- Idle compaction setting for Claude Code terminals (Settings > Terminal).
- Run button restarts a command that is already running.
- Dev and release builds run side by side, and updates keep running terminal sessions.

### Fixed
- "More models" submenu closing while moving the mouse diagonally toward it.
- Subagents marking a Claude terminal as busy.
- Fullscreen from a maximized window leaving a taskbar gap on Windows.
- Tile resize selecting page text on macOS.
- Markdown html images ignoring width and height.

## [0.2.6]

### Added
- Claude agent bar fed by a Claude Code mod: live model, context, effort, permission mode, busy/waiting/idle status and the session's real slash commands.
- Claude model picker loaded from the Claude Code model catalog: main models up front, older ones under "More models", plus a 200k / 1M context selector.
- Argument choices for slash commands such as /output-style.
- Agent usage status bar.
- Kimi Code support in the agent bar.
- Pi agent: resume sessions, notifications, context window selector and shared linked context.
- Tabs in notes.
- Create files and folders from the file tree.
- Markdown preview in the file editor, with tables, headings and inline marks.
- Image previews in the diff viewer and file editor.
- Nested repository changes in the git tab.
- Notifications from background tabs, opening the notified tab on click.
- App state stored in SQLite with atomic, durable writes.

### Fixed
- Workspace writes racing each other and state saving to the wrong tab.
- Agent bar disappearing after a Pi compaction.
- Kimi transcript not scrolling with the mouse wheel on Windows.
- Toolbox tools without a product code breaking IDE detection.
- Image paste into terminal tiles.

## [0.2.5]

### Added
- Tabbed file editor: open files from the Files tab, the diff viewer, the git tab or a canvas tile, with unsaved-change guards.
- Browse and diff the files of any commit straight from the git history tab.
- pi agent support in the agent bar: identity detection, model catalog and effort pickers, plus mouse and word-delete handling.
- Hover-reveal minimap with drag panning and agent state colors, an always-visible toggle and a configurable corner.
- General settings tab, and appearance toggles for individual tile header elements.
- Configurable notification corner and target display.
- Opus 5 in the model switcher, now grouped by family.
- Open a tile working directory in the detected IDE from the context menu.
- Context menu on git sections and folders, and copy tile title from the header menu.
- Filter commits in the git history tab.
- Background fetch that flags pending pulls on tracked branches.
- Notification when a Claude compact finishes.
- go-task Taskfile targets detected for the run and build buttons.
- Dotfiles shown in the file tree, with git-ignored entries dimmed, and broader file icon coverage.
- Empty diffs now show the file content plus a line-separator notice.

### Fixed
- Ctrl+C not killing running processes in terminal tiles.
- Terminal restart racing the old pty on the same key.
- Diff panel rendering underneath the minimap.
- Notification window appearing in the Alt+Tab switcher.
- Agent bar stuck hidden after the resume transcript menu, and prompts interrupting Claude instead of queueing.
- Main window not restoring when clicking a tray notification.
- Claude identified from sidecar state and footer model.
- Console windows flashing when git and docker commands ran.
- Git and docker commands freezing the UI by blocking the async runtime.
- Agent bar context warning now triggers earlier.

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
