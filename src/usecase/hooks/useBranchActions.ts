import React from 'react';

import type { ToastTask } from '~/usecase/util/toasts';
import type { LocalBranch, BranchSnapshot, RepoOperationAction } from '~/domain/interfaces/git.interface';
import { errorText, showToast, toastTask } from '~/usecase/util/toasts';
import {
  gitFetch,
  gitCheckout,
  gitOperation,
  gitStashCheckout,
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

export type BranchOp = 'fetch' | 'update' | 'push' | 'checkout' | 'merge' | 'rebase' | 'delete' | 'upstream' | 'operation';

type CheckoutRunner = (path: string, branch: string) => Promise<BranchSnapshot>;

const BLOCKED_BY_CHANGES = 'would be overwritten by checkout';

const OPERATION_TEXT: Record<RepoOperationAction, { pending: string; done: string }> = {
  skip: { pending: 'Skipping commit...', done: 'Skipped commit' },
  abort: { pending: 'Aborting...', done: 'Aborted' },
  continue: { pending: 'Continuing...', done: 'Continued' }
};

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

  const pull = (rebase: boolean) => {
    const before = head(snapshot);
    return track({ op: 'update' }, gitUpdateBranch(cwd, rebase), {
      pending: rebase ? 'Pulling with rebase...' : 'Pulling...',
      failed: 'Pull failed',
      done: (snap) => ({ kind: 'success', message: summarize('update', before, head(snap)) })
    });
  };

  const update = () => pull(false);
  const pullRebase = () => pull(true);

  const push = () => {
    const before = head(snapshot);
    return track({ op: 'push' }, gitPushCurrent(cwd), {
      pending: 'Pushing...',
      failed: 'Push failed',
      done: (snap) => ({ kind: 'success', message: summarize('push', before, head(snap)) })
    });
  };

  const switchTo = (branch: string, runner: CheckoutRunner, previous: string | null): Promise<BranchSnapshot | null> => {
    const name = shortName(snapshot, branch);
    const stashing = runner === gitStashCheckout;
    const failed = `Checkout of ${name} failed`;

    return track({ op: 'checkout', branch }, runner(cwd, branch), {
      failed,
      pending: stashing ? `Stashing changes and checking out ${name}...` : `Checking out ${name}...`,
      failure: (err) => {
        const detail = errorText(err);
        if (stashing && detail.startsWith('Checked out')) {
          return { kind: 'error', message: `Checked out ${name}, but local changes conflict`, detail };
        }
        if (stashing || !detail.includes(BLOCKED_BY_CHANGES)) return { kind: 'error', message: failed, detail };
        return {
          detail,
          kind: 'error',
          message: `Local changes block checkout of ${name}`,
          action: { label: 'Stash & checkout', run: () => void switchTo(branch, gitStashCheckout, previous) }
        };
      },
      done: (snap) => ({
        kind: 'success',
        message: `Checked out ${snap.current ?? name}`,
        action:
          previous && previous !== snap.current
            ? { label: 'Rollback', run: () => void switchTo(previous, gitCheckout, snap.current) }
            : undefined
      })
    });
  };

  const checkout = (branch: string) => switchTo(branch, gitCheckout, current);

  const checkoutAndRebase = (branch: string) => {
    const base = current;
    if (!base) return Promise.resolve(null);
    const task = gitCheckout(cwd, branch).then(() => gitRebaseOnto(cwd, base));
    return track({ op: 'rebase', branch }, task, {
      pending: `Rebasing ${branch} onto ${base}...`,
      failed: `Rebase of ${branch} onto ${base} failed`,
      done: () => ({ kind: 'success', message: `Checked out ${branch} and rebased it onto ${base}` })
    });
  };

  const operation = (action: RepoOperationAction) =>
    track({ op: 'operation' }, gitOperation(cwd, action), {
      pending: OPERATION_TEXT[action].pending,
      failed: `Could not ${action}`,
      done: () => ({ kind: 'success', message: OPERATION_TEXT[action].done })
    });

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
    operation,
    pullRebase,
    checkoutAndRebase,
    setUpstream,
    toggleFavorite
  };
};
