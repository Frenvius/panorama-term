import type React from 'react';
import type { ClaudeState, GridFrame } from '~/domain/interfaces/pty.interface';
import type { AgentType } from './parse';

export interface AgentBarProps {
  tileId: string;
  sessionId: string;
  active: boolean;
  send: (data: string) => boolean;
  getLines: () => string[];
  getFrame: () => GridFrame | null;
  getStructured: () => ClaudeState | null;
  focusTerminal: () => void;
  onAgentActive?: (type: AgentType | null) => void;
}

export type SuggestTrigger = { kind: 'slash' | 'model' | 'effort'; query: string } | null;

export type ContentPart = { type: 'text'; content: string } | { type: 'image'; path: string };

export interface DraftState {
  text: string;
  images: string[];
}

export interface ParsedStatus {
  mode?: string;
  model?: string;
  focused?: boolean;
  progress?: number;
  contextInfo?: string;
}

export interface FooterRead {
  status: string[];
  uiPresent: boolean;
  questionMode: boolean;
  model?: { model: string; contextInfo?: string };
}

export interface PromptSuggestion {
  id: string;
  display: string;
  color?: string;
  subtext?: string;
  takesArg?: boolean;
  icon: 'cmd' | 'model' | 'effort';
}

export interface AgentSuggestHandle {
  handleKeyDown: (e: React.KeyboardEvent) => boolean;
}
