import type { CatalogModel } from '~/domain/interfaces/pty.interface';

export interface AgentSlashCommand {
  name: string;
  desc: string;
  aliases?: string[];
  takesArg?: boolean;
}

export interface AgentModel {
  name: string;
  desc: string;
  aliases?: string[];
}

export const HISTORY_KEY = 'agent:prompthistory';
export const BPM_START = '\x1b[200~';
export const BPM_END = '\x1b[201~';

export const draftKey = (tileId: string): string => `agent:draft:${tileId}`;

export const FALLBACK_MODEL_CATALOG: CatalogModel[] = [
  { id: 'claude-opus-5-5', name: 'Opus 5.5', section: 'main' },
  { id: 'claude-fable-5-1', name: 'Fable 5.1', section: 'main' },
  { id: 'claude-sonnet-5-5', name: 'Sonnet 5.5', section: 'main' },
  { id: 'claude-haiku-5-5', name: 'Haiku 5.5', section: 'main' },
  { id: 'claude-haiku-4-5-20251001', name: 'Haiku 4.5', section: 'more' },
  { id: 'claude-sonnet-5', name: 'Sonnet 5', section: 'more' },
  { id: 'claude-opus-5', name: 'Opus 5', section: 'more' },
  { id: 'claude-fable-5', name: 'Fable 5', section: 'more' },
  { id: 'claude-opus-4-8', name: 'Opus 4.8', section: 'more' },
  { id: 'claude-opus-4-7', name: 'Opus 4.7', section: 'more' },
  { id: 'claude-opus-4-6', name: 'Opus 4.6', section: 'more' },
  { id: 'claude-sonnet-4-6', name: 'Sonnet 4.6', section: 'more' }
];

export const MODEL_CONTEXT_VARIANTS = [
  { suffix: '', title: '200k context' },
  { suffix: '[1m]', title: '1M context' }
] as const;

export const EFFORT_LEVELS = [
  { id: 'low', desc: 'Fastest, shallow reasoning', color: '#4ade80' },
  { id: 'medium', desc: 'Balanced', color: '#a3e635' },
  { id: 'high', desc: 'Default', color: '#38bdf8' },
  { id: 'xhigh', desc: 'Deeper reasoning', color: '#fb923c' },
  { id: 'max', desc: 'Smartest, slowest', color: '#f87171' },
  { id: 'ultracode', desc: 'xhigh + workflows', color: '#a855f7' }
] as const;

export const CLAUDE_MODELS: AgentModel[] = [
  { name: 'default', desc: 'Recommended default model' },
  { name: 'opus', desc: 'Latest Claude Opus' },
  { name: 'sonnet', desc: 'Latest Claude Sonnet' },
  { name: 'haiku', desc: 'Latest Claude Haiku' },
  { name: 'opusplan', desc: 'Opus for planning, Sonnet for execution' },
  { name: 'claude-opus-5-5', desc: 'Opus 5.5 - latest' },
  { name: 'claude-opus-5', desc: 'Opus 5' },
  { name: 'claude-opus-4-8', desc: 'Opus 4.8' },
  { name: 'claude-opus-4-7', desc: 'Opus 4.7' },
  { name: 'claude-opus-4-6', desc: 'Opus 4.6' },
  { name: 'claude-sonnet-5', desc: 'Sonnet 5' },
  { name: 'claude-sonnet-4-6', desc: 'Sonnet 4.6' },
  { name: 'claude-haiku-4-5', desc: 'Haiku 4.5 - fastest' },
  { name: 'claude-fable-5-1', desc: 'Fable 5.1 - latest' },
  { name: 'claude-fable-5', desc: 'Fable 5' }
];

export const CLAUDE_SLASH_COMMANDS: AgentSlashCommand[] = [
  { name: '/clear', desc: 'New conversation', aliases: ['/reset', '/new'] },
  { name: '/compact', desc: 'Compact conversation with optional focus', takesArg: true },
  { name: '/resume', desc: 'Resume a session or open picker' },
  { name: '/branch', desc: 'Fork current conversation', takesArg: true },
  { name: '/rename', desc: 'Rename current session', takesArg: true },
  { name: '/exit', desc: 'Exit the CLI' },
  { name: '/model', desc: 'Select/change AI model' },
  { name: '/effort', desc: 'Set reasoning effort level', takesArg: true },
  { name: '/config', desc: 'Open Settings interface' },
  { name: '/fast', desc: 'Toggle fast mode' },
  { name: '/theme', desc: 'Change color theme' },
  { name: '/plan', desc: 'Enter plan mode' },
  { name: '/diff', desc: 'View uncommitted changes' },
  { name: '/rewind', desc: 'Rewind conversation/code to prior point' },
  { name: '/review', desc: 'Review a pull request', takesArg: true },
  { name: '/context', desc: 'Visualize context usage' },
  { name: '/cost', desc: 'Show token usage stats' },
  { name: '/usage', desc: 'Show plan limits/rate limits' },
  { name: '/help', desc: 'Show help' },
  { name: '/doctor', desc: 'Diagnose installation' },
  { name: '/init', desc: 'Initialize CLAUDE.md' },
  { name: '/memory', desc: 'Edit memory files' },
  { name: '/permissions', desc: 'Manage tool permissions' },
  { name: '/skills', desc: 'List available skills' },
  { name: '/mcp', desc: 'Manage MCP server connections' },
  { name: '/hooks', desc: 'View hook configurations' },
  { name: '/simplify', desc: 'Code review for quality/efficiency' },
  { name: '/loop', desc: 'Run a prompt repeatedly', takesArg: true },
  { name: '/schedule', desc: 'Create/manage scheduled tasks', takesArg: true },
  { name: '/security-review', desc: 'Security review of pending changes' }
];

export const KIMI_SLASH_COMMANDS: AgentSlashCommand[] = [
  { name: '/new', desc: 'Start a fresh session in the current workspace', aliases: ['/clear'] },
  { name: '/sessions', desc: 'Browse and resume sessions', aliases: ['/resume'] },
  { name: '/model', desc: 'Switch LLM model' },
  { name: '/effort', desc: 'Switch thinking effort', aliases: ['/thinking'], takesArg: true },
  { name: '/plan', desc: 'Toggle plan mode' },
  { name: '/permission', desc: 'Select permission mode' },
  { name: '/ask-when-needed', desc: 'Routine edits run automatically; risky actions still ask', aliases: ['/yolo', '/yes'] },
  { name: '/never-ask', desc: 'Never interrupts you; everything runs automatically', aliases: ['/auto'] },
  { name: '/compact', desc: 'Compact the conversation context', takesArg: true },
  { name: '/usage', desc: 'Show session tokens + context window + plan quotas' },
  { name: '/status', desc: 'Show current session and runtime status' },
  { name: '/goal', desc: 'Start or manage an autonomous goal', takesArg: true },
  { name: '/btw', desc: 'Ask a forked side agent a question', takesArg: true },
  { name: '/swarm', desc: 'Toggle swarm mode or run one task in swarm mode', takesArg: true },
  { name: '/tower', desc: 'Report tower status or toggle tower mode', takesArg: true },
  { name: '/tasks', desc: 'Browse background tasks', aliases: ['/task'] },
  { name: '/title', desc: 'Set or show session title', aliases: ['/rename'], takesArg: true },
  { name: '/fork', desc: 'Fork the current session into a copy' },
  { name: '/undo', desc: 'Withdraw the last prompt from the transcript' },
  { name: '/copy', desc: 'Copy the last assistant message to the clipboard' },
  { name: '/init', desc: 'Analyze the codebase and generate AGENTS.md' },
  { name: '/add-dir', desc: 'Add or list an additional workspace directory', takesArg: true },
  { name: '/mcp', desc: 'Show MCP server status' },
  { name: '/plugins', desc: 'Manage plugins' },
  { name: '/provider', desc: 'Manage AI providers', aliases: ['/providers'] },
  { name: '/secondary-model', desc: 'Configure the secondary model for subagents', aliases: ['/subagent-model'] },
  { name: '/experiments', desc: 'Manage experimental features', aliases: ['/experimental'] },
  { name: '/settings', desc: 'Open TUI settings', aliases: ['/config'] },
  { name: '/theme', desc: 'Set the terminal UI theme', takesArg: true },
  { name: '/editor', desc: 'Set the external editor for Ctrl-G' },
  { name: '/reload', desc: 'Reload config.toml settings plus tui.toml UI preferences' },
  { name: '/reload-tui', desc: 'Reload only tui.toml UI preferences' },
  { name: '/export-md', desc: 'Export current session as a Markdown file', aliases: ['/export'] },
  { name: '/export-debug-zip', desc: 'Export current session as a debug ZIP archive' },
  { name: '/web', desc: 'Open the current session in the Web UI' },
  { name: '/remote-control', desc: 'Open the session through Kimi Remote Control', aliases: ['/rc'] },
  { name: '/login', desc: 'Select a platform and authenticate' },
  { name: '/logout', desc: 'Log out of a configured provider', aliases: ['/disconnect'] },
  { name: '/feedback', desc: 'Send feedback to make Kimi Code better', aliases: ['/bug'] },
  { name: '/help', desc: 'Show available commands and shortcuts', aliases: ['/h', '/?'] },
  { name: '/version', desc: 'Show version information' },
  { name: '/exit', desc: 'Exit the application', aliases: ['/quit', '/q'] }
];

export const ANTIGRAVITY_SLASH_COMMANDS: AgentSlashCommand[] = [
  { name: '/add-dir', desc: 'Add a directory to the workspace', takesArg: true },
  { name: '/agents', desc: 'List available custom agents' },
  { name: '/artifact', desc: 'View and review artifacts', takesArg: true },
  { name: '/btw', desc: 'Ask a side question without interrupting the current task', takesArg: true },
  { name: '/changelog', desc: 'Show release notes and changes' },
  { name: '/clear', desc: 'Clear conversation and start a new one', aliases: ['/new'] },
  { name: '/codesearch', desc: 'Search code in the workspace', aliases: ['/cs', '/search'], takesArg: true },
  { name: '/config', desc: 'Open settings panel', aliases: ['/settings'] },
  { name: '/context', desc: 'Visualize current context usage' },
  { name: '/copy', desc: 'Copy the last planner response to the clipboard' },
  { name: '/credits', desc: 'Show remaining G1 credits and purchase link' },
  { name: '/diff', desc: 'View uncommitted changes and per-turn diffs' },
  { name: '/exit', desc: 'Exit the CLI', aliases: ['/quit'] },
  { name: '/feedback', desc: 'Submit qualitative feedback to improve the agent', takesArg: true },
  { name: '/fork', desc: 'Create a branch of the current conversation', aliases: ['/branch'], takesArg: true },
  { name: '/help', desc: 'Show available commands and keybindings' },
  { name: '/hooks', desc: 'Manage hook configurations for tool events' },
  { name: '/keybindings', desc: 'Set custom keybindings' },
  { name: '/logout', desc: 'Log out' },
  { name: '/mcp', desc: 'Manage MCP servers' },
  { name: '/model', desc: 'Set a model', takesArg: true },
  { name: '/open', desc: 'Open a file or view opened/edited files', takesArg: true },
  { name: '/permissions', desc: 'Manage tool permissions' },
  { name: '/rename', desc: 'Rename the current conversation', takesArg: true },
  { name: '/resume', desc: 'Browse and resume past conversations', aliases: ['/switch', '/conversation'] },
  { name: '/rewind', desc: 'Rewind conversation to a previous message', aliases: ['/undo'] },
  { name: '/skills', desc: 'List available skills' },
  { name: '/statusline', desc: 'Toggle the statusline' },
  { name: '/tasks', desc: 'View background tasks' },
  { name: '/title', desc: 'Toggle custom terminal window title', takesArg: true },
  { name: '/usage', desc: 'View model quota usage', aliases: ['/quota'] },
  { name: '/goal', desc: 'Run until the specified goal is completely finished.', takesArg: true },
  { name: '/schedule', desc: 'Run an instruction on a recurring schedule or as a one-time timer.', takesArg: true },
  { name: '/plan', desc: 'Plan carefully before executing a task.' },
  { name: '/grill-me', desc: 'Interview me to align on a plan.' },
  { name: '/teamwork-preview', desc: 'Invoke a team of agents to autonomously tackle large projects.' },
  { name: '/learn', desc: 'Reflect on recent successes or corrections to capture reusable skills or rules.' },
  { name: '/agy-customizations', desc: 'Comprehensive guide and reference for the Antigravity Customization System.' },
  { name: '/antigravity-guide', desc: 'Provides a comprehensive guide, quick reference, and sitemap for Google Antigravity.' }
];
