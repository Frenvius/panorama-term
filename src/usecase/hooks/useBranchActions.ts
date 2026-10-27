import React from 'react';

import type { ToastTask } from '~/usecase/util/toasts';
import type { LocalBranch, BranchSnapshot } from '~/domain/interfaces/git.interface';
import { showToast, toastTask } from '~/usecase/util/toasts';
import {
  gitFetch,
  gitCheckout,
  gitResolveRef,
  gitRebaseOnto,
  gitMergeBranch,
  gitSetUpstream,
  gitPushCurrent,
  gitCreateBranch,
  gitDeleteBranch,
  gitRenameBranch,
  gitUpdateBranch,
  gitToggleBranchFavorite
} from '~/adapter/git/git.client';

export type BranchOp = 'fetch' | 'update' | 'push' | 'checkout' | 'merge' | 'rebase' | 'delete' | 'upstream';

export interface BranchBusy {
  op: BranchOp;
  branch?: string;
}

interface BranchSync {
  onSnapshot: (snap: BranchSnapshot) => void;
  onError: (error: string | null) => void;
}

const head = (snap: BranchSnapshot | null): LocalBranch | undefined => snap?.local.find((b) => b.is_current);

const commits = (n: number): string => `${n} commit${n === 1 ? '' : 's'}`;

const shortName = (snap: BranchSnapshot | null, branch: string): string => {
  const remote = snap?.remotes.find((r) => `${r.remote}/${r.branch}` === branch);
  return remote ? remote.branch : branch;
};

const summarize = (kind: 'update' | 'push', before?: LocalBranch, after?: LocalBranch): string => {
  const moved = kind === 'update' ? (before?.behind ?? 0) - (after?.behind ?? 0) : (before?.ahead ?? 0) - (after?.ahead ?? 0);
  const target = after?.upstream ?? before?.upstream ?? 'remote';
  if (moved <= 0) return 'Already up to date';
  return kind === 'update' ? `Pulled ${commits(moved)} from ${target}` : `Pushed ${commits(moved)} to ${target}`;
};

export const useBranchActions = (cwd: string, snapshot: BranchSnapshot | null, sync: BranchSync) => {
  const [busy, setBusy] = React.useState<BranchBusy | null>(null);
  const current = snapshot?.current ?? null;

  const apply = (snap: BranchSnapshot): BranchSnapshot => {
    sync.onSnapshot(snap);
    sync.onError(null);
    return snap;
  };

  const track = (state: BranchBusy, task: Promise<BranchSnapshot>, labels: ToastTask<BranchSnapshot>) => {
    setBusy(state);
    return toastTask(task.then(apply), labels)
      .catch(() => null)
      .finally(() => setBusy(null));
  };

  const fetchAll = () =>
    track({ op: 'fetch' }, gitFetch(cwd), {
      pending: 'Fetching...',
      failed: 'Fetch failed',
      done: () => ({ kind: 'success', message: 'Fetched all remotes' })
    });

  const update = () => {
    const before = head(snapshot);
    return track({ op: 'update' }, gitUpdateBranch(cwd, false), {
      pending: 'Pulling...',
      failed: 'Pull failed',
      done: (snap) => ({ kind: 'success', message: summarize('update', before, head(snap)) })
    });
  };

  const push = () => {
    const before = head(snapshot);
    return track({ op: 'push' }, gitPushCurrent(cwd), {
      pending: 'Pushing...',
      failed: 'Push failed',
      done: (snap) => ({ kind: 'success', message: summarize('push', before, head(snap)) })
    });
  };

  const checkout = (branch: string, previous = current) => {
    const name = shortName(snapshot, branch);
    return track({ op: 'checkout', branch }, gitCheckout(cwd, branch), {
      pending: `Checking out ${name}...`,
      failed: `Checkout of ${name} failed`,
      done: (snap) => ({
        kind: 'success',
        message: `Checked out ${snap.current ?? name}`,
        action:
          previous && previous !== snap.current
            ? { label: 'Rollback', run: () => void checkout(previous, snap.current) }
            : undefined
      })
    });
  };

  const merge = (branch: string) => {
    const name = shortName(snapshot, branch);
    return track({ op: 'merge', branch }, gitMergeBranch(cwd, branch), {
      pending: `Merging ${name}...`,
      failed: `Merge of ${name} failed`,
      done: () => ({ kind: 'success', message: `Merged ${name} into ${current ?? 'current branch'}` })
    });
  };

  const rebase = (branch: string) => {
    const name = shortName(snapshot, branch);
    return track({ op: 'rebase', branch }, gitRebaseOnto(cwd, branch), {
      pending: `Rebasing onto ${name}...`,
      failed: `Rebase onto ${name} failed`,
      done: () => ({ kind: 'success', message: `Rebased ${current ?? 'current branch'} onto ${name}` })
    });
  };

  const setUpstream = (branch: string, upstream: string | null) =>
    track({ op: 'upstream', branch }, gitSetUpstream(cwd, branch, upstream), {
      pending: 'Updating tracked branch...',
      failed: 'Could not change tracked branch',
      done: () => ({
        kind: 'success',
        message: upstream ? `${branch} now tracks ${upstream}` : `${branch} no longer tracks a remote branch`
      })
    });

  const restore = (branch: string, hash: string) =>
    track({ op: 'checkout', branch }, gitCreateBranch(cwd, branch, false, false, hash), {
      pending: `Restoring ${branch}...`,
      failed: `Could not restore ${branch}`,
      done: () => ({ kind: 'success', message: `Restored ${branch} at ${hash.slice(0, 7)}` })
    });

  const remove = async (branch: string, isRemote: boolean) => {
    const tip = isRemote ? null : await gitResolveRef(cwd, `refs/heads/${branch}`).catch(() => null);
    return track({ op: 'delete', branch }, gitDeleteBranch(cwd, branch, isRemote), {
      pending: `Deleting ${branch}...`,
      failed: `Could not delete ${branch}`,
      done: () => ({
        kind: 'success',
        message: isRemote ? `Deleted remote branch ${branch}` : `Deleted branch ${branch}`,
        action: tip ? { label: 'Restore', run: () => void restore(branch, tip) } : undefined
      })
    });
  };

  const toggleFavorite = (branch: string) =>
    toastTask(gitToggleBranchFavorite(cwd, branch).then(apply), {
      pending: 'Saving favorite...',
      failed: 'Could not change favorite',
      done: () => null
    }).catch(() => null);

  const create = (name: string, checkoutNew: boolean, overwrite: boolean, startPoint?: string) =>
    gitCreateBranch(cwd, name, checkoutNew, overwrite, startPoint)
      .then(apply)
      .then((snap) => {
        showToast({ kind: 'success', message: checkoutNew ? `Created and checked out ${name}` : `Created branch ${name}` });
        return snap;
      });

  const rename = (oldName: string, newName: string) =>
    gitRenameBranch(cwd, oldName, newName)
      .then(apply)
      .then((snap) => {
        showToast({ kind: 'success', message: `Renamed ${oldName} to ${newName}` });
        return snap;
      });

  return {
    busy,
    push,
    merge,
    update,
    create,
    rename,
    rebase,
    remove,
    checkout,
    fetchAll,
    setUpstream,
    toggleFavorite
  };
};
