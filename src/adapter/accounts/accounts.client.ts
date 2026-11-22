import type {
  AccountListReply,
  AccountActivateReply,
  AccountAddStartReply,
  AgentAccountKind,
  PerTileAccountKind,
  TileAccountsReply,
  AccountLoginStatusReply
} from '~/domain/interfaces/account.interface';

import { SIDECAR_HTTP } from '~/adapter/pty/sidecar.client';

interface OkResponse {
  ok: boolean;
  error?: { code: string; message: string };
}

export class AccountsError extends Error {
  code?: string;
  constructor(message: string, code?: string) {
    super(message);
    this.name = 'AccountsError';
    this.code = code;
  }
}

const OUTDATED = 'Panorama brain is outdated: restart it to enable accounts';

const post = async <T>(command: string, body: unknown): Promise<T> => {
  let res: Response;
  try {
    res = await fetch(`${SIDECAR_HTTP}/accounts/${command}`, { method: 'POST', body: JSON.stringify(body) });
  } catch {
    throw new Error(OUTDATED);
  }
  if (!res.ok) throw new Error(OUTDATED);
  let json: OkResponse & T;
  try {
    json = (await res.json()) as OkResponse & T;
  } catch {
    throw new Error(OUTDATED);
  }
  if (!json.ok) throw new AccountsError(json.error?.message ?? command, json.error?.code);
  return json;
};

const discard = (): void => undefined;

export const listAccounts = (): Promise<AccountListReply> => post('list', {});

interface StartLoginOpts {
  tileId: string;
  agent: AgentAccountKind;
}

export const startAccountLogin = (opts: StartLoginOpts): Promise<AccountAddStartReply> => post('add-start', opts);

export const getAccountLoginStatus = (pendingId: string): Promise<AccountLoginStatusReply> =>
  post('add-status', { pendingId });

export const cancelAccountLogin = (pendingId: string): Promise<void> => post('add-cancel', { pendingId }).then(discard);

interface AccountRef {
  id: string;
  agent: AgentAccountKind;
}

export const removeAccount = (ref: AccountRef): Promise<void> => post('remove', ref).then(discard);

export const renameAccount = (ref: AccountRef & { label: string }): Promise<void> => post('rename', ref).then(discard);

export const setDefaultAccount = (ref: { id: string; agent: PerTileAccountKind }): Promise<void> =>
  post('set-default', ref).then(discard);

export const activateAgyAccount = (id: string): Promise<AccountActivateReply> =>
  post('activate', { agent: 'antigravity', id });

export const getTileAccounts = (tileId: string): Promise<TileAccountsReply> => post('tile-get', { tileId });

interface SetTileAccountOpts {
  id: string;
  tileId: string;
  agent: PerTileAccountKind;
}

export const setTileAccount = (opts: SetTileAccountOpts): Promise<void> => post('tile-set', opts).then(discard);
