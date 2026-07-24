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

export const MODEL_QUICK_SWITCHES = [
  { id: 'claude-fable-5', title: 'Fable 5' },
  { id: 'claude-opus-5', title: 'Opus 5' },
  { id: 'claude-opus-4-8', title: 'Opus 4.8' },
  { id: 'claude-opus-4-7', title: 'Opus 4.7' },
  { id: 'claude-opus-4-6', title: 'Opus 4.6' },
  { id: 'claude-sonnet-5', title: 'Sonnet 5' },
  { id: 'claude-sonnet-4-6', title: 'Sonnet 4.6' }
] as const;

export const MODEL_CONTEXT_VARIANTS = [
  { suffix: '', title: 'default' },
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
  { name: 'claude-opus-5', desc: 'Opus 5 - latest' },
  { name: 'claude-opus-4-8', desc: 'Opus 4.8' },
  { name: 'claude-opus-4-7', desc: 'Opus 4.7' },
  { name: 'claude-opus-4-6', desc: 'Opus 4.6' },
  { name: 'claude-sonnet-5', desc: 'Sonnet 5' },
  { name: 'claude-sonnet-4-6', desc: 'Sonnet 4.6' },
  { name: 'claude-haiku-4-5', desc: 'Haiku 4.5 - fastest' },
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
