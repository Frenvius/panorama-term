import type React from 'react';
import type { ClaudeState } from '~/domain/interfaces/pty.interface';

export interface AgentBarProps {
  tileId: string;
  active: boolean;
  send: (data: string) => void;
  getLines: () => string[];
  getStructured: () => ClaudeState | null;
  focusTerminal: () => void;
  onClaudeActive?: (active: boolean) => void;
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
