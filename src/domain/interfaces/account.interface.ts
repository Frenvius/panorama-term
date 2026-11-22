export type AgentAccountKind = 'antigravity' | 'claude' | 'codex';

export type PerTileAccountKind = 'claude' | 'codex';

export interface AgentAccount {
  id: string;
  label: string;
  system: boolean;
  createdAt: number;
  agent: AgentAccountKind;
  email: string | null;
}

export interface AccountDefaults {
  codex: string;
  claude: string;
}

export interface AccountListReply {
  ok: boolean;
  accounts: AgentAccount[];
  defaults: AccountDefaults;
  liveAgy: string | null;
}

export interface AccountAddStartReply {
  ok: boolean;
  command: string;
  pendingId: string;
}

export type AccountLoginState = 'waiting' | 'done' | 'failed';

export interface AccountLoginStatusReply {
  ok: boolean;
  error?: string;
  state: AccountLoginState;
  account?: AgentAccount;
}

export type AgyTileStatus = 'idle' | 'busy' | 'waiting' | null;

export interface AgyTile {
  tileId: string;
  status: AgyTileStatus;
  conversationId: string | null;
}

export interface AccountActivateReply {
  ok: boolean;
  tiles: AgyTile[];
}

export interface TileAccountsReply {
  ok: boolean;
  codex: string;
  claude: string;
}
