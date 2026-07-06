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
}

export type PtyServerMessage = PtyReadyMessage | PtyExitMessage | PtyCwdMessage;

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
