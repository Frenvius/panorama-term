export interface PtyReadyMessage {
  t: 'ready';
  reused: boolean;
  cols: number;
  rows: number;
  resumeId: string | null;
}

export interface PtyExitMessage {
  t: 'exit';
}

export interface PtyCwdMessage {
  t: 'cwd';
  cwd: string;
  branch?: string | null;
}

export interface ClaudeState {
  model?: string;
  mode?: string;
  permissionMode?: string;
  contextTokens?: number;
  effort?: string;
  defaultModel?: string;
  status?: string;
  contextPercent?: number;
  contextWindow?: number;
  costUsd?: number;
  sessionName?: string;
  outputStyle?: string;
  thinking?: boolean;
  rateFiveHour?: number;
  rateSevenDay?: number;
}

export interface PtyClaudeMessage extends ClaudeState {
  t: 'claude';
}

export interface PtyClipboardMessage {
  t: 'clipboard';
  text: string;
}

export interface PtyTitleMessage {
  t: 'title';
  title: string;
}

export interface PtyNotifyMessage {
  t: 'notify';
  title: string;
  body: string;
}

export interface AgentEvent {
  event: string;
  sessionId?: string;
  project?: string;
  query?: string;
  response?: string;
  toolName?: string;
  message?: string;
}

export interface PtyAgentEventMessage extends AgentEvent {
  t: 'agentEvent';
}

export interface PtyProgressMessage {
  t: 'progress';
  state: number;
  pct: number;
}

export type PtyServerMessage =
  | PtyReadyMessage
  | PtyExitMessage
  | PtyCwdMessage
  | PtyClaudeMessage
  | PtyClipboardMessage
  | PtyTitleMessage
  | PtyNotifyMessage
  | PtyAgentEventMessage
  | PtyProgressMessage;

export interface GridFrame {
  rows: number;
  cols: number;
  cursorRow: number;
  cursorCol: number;
  cursorHidden: boolean;
  mouseMode: number;
  offset: number;
  lines: string[];
  attrs: Uint32Array;
}
