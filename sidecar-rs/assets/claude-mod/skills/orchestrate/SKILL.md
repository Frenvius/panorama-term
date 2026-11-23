---
name: orchestrate
description: Coordinate a multi-agent run in Panorama - split work into tasks, start CLI agent workers (claude, codex, opencode, kimi, pi, antigravity) in their own git worktrees, react to their reports, verify, retry, resolve merge conflicts, integrate, and report. Use when the user asks to orchestrate, delegate, fan out, or parallelize work across worker agents, or to run tasks overnight with cheaper models.
allowed-tools: Bash({cli} orch:*), PowerShell({cli} orch:*)
---

# Orchestrating workers in Panorama

You are the coordinator. You plan, start workers, read their results, and decide what happens next.
The Panorama brain records every step in an event log, runs verification, and commits and merges
the work. Unless the run has autonomy switched on (section 7), it never starts a worker on its own:
every dispatch is your decision.

Do not implement the tasks yourself. Small glue fixes are fine when a worker is clearly the wrong
tool, but tell the user.

All commands go through the Panorama CLI:

```
{cli} orch <command> [--flags]
```

Replies are JSON `{"ok":true,...}`. Exit codes: 0 ok, 2 bad usage, 3 rejected by the run state (the
JSON says why), 4 brain unreachable. Pass `--pretty` to read a reply comfortably.

## 1. Plan with the user

Do not orchestrate small work. One task that one agent can finish quickly is cheaper done directly.
Orchestration pays off for large or parallelizable work - several independent tasks or work that
benefits from cheaper worker models.

Agree on these before creating anything:

- **Repo** and base ref (default `HEAD`). Workers branch from it.
- **Tasks**. Each one is a self-contained unit of work a cheaper model can finish alone. Assign a
  role to each task based on what it does:
  - `explore` - read, search, map code, answer questions; no edits.
  - `implement` - write or change code to a spec.
  - `test` - write and run tests for finished work.
  - `review` - read a diff and report problems; no edits.
  - `mechanical` - renames, formatting, boilerplate, moves.
- **Workers**. The brain picks the agent from the role chain in Settings > Orchestration. Pass
  `--escalation`/`--profile` only to override when the user names a specific agent.
  Built-in profiles: `claude` (models `haiku`, `sonnet`, `opus`), `codex`, `opencode` (model is `provider/model`), `kimi`, `pi`, `antigravity` (exe `agy`).
- **Verification**. Commands that exit 0 when a task is done, such as tests, a typecheck or a build.

Good tasks:

- The spec is self-contained. The worker sees only its brief, never this conversation. Name the
  files, the expected behavior, and the constraints.
- Tasks that run in parallel touch different files. Overlapping edits end in merge conflicts.
- Real ordering goes in `--dep`. A dependent task starts from the integration branch, so it sees
  the work of its integrated dependencies.
- `--verify` commands run in the task's worktree through the system shell (PowerShell on Windows).
  Each must exit 0. Keep them fast and scoped to the task.

## 2. Create the run and its tasks

```
{cli} orch run-create --repo <abs path> --title "<title>"
```

Optional: `--base-ref <ref>`, `--setup "<command run in each new worktree>"` (e.g. `bun install`),
`--max-workers 4`, `--max-attempts 3`, `--dispatch-timeout-min 60`, `--verify-timeout-min 15`,
`--stall-quiet-min 5`, `--max-operational-attempts 6`, `--autonomy <list>` (section 7). Note the
returned `run.id`.

The CLI records this terminal as the coordinator tile (from `PANORAMA_TILE_ID`), so the brain can
type a short notice into it when messages arrive while you are idle.

```
{cli} orch task-create --run-id <run> --title "<short title>" --spec "<full spec>" \
  --acceptance "<what done means>" --verify "<command>" --dep t1 --role implement
```

- `--role` sets the task role: `explore`, `implement`, `test`, `review`, `mechanical`. Omit to
  default to `implement`. The brain resolves the agent chain from Settings > Orchestration.
- Use `--escalation`/`--profile` only to override the role chain for a specific task.
- `--spec-file <path>` reads a long spec from a file.
- `--verify` and `--dep` repeat. `--escalation` repeats when overriding the role chain.
- An escalation entry is `profile` or `profile:model`. The list is a preference order, not an
  attempt ladder: operational failures (quota, rate limit, auth, crash, launch) fall back to the next
  eligible entry without spending attempts; quality failures (the worker gave up, verification failed)
  move past the entry that failed and spend an attempt. A rejected entry never comes back.
- `--review` keeps the task in `verifying` after its checks pass, until you approve it (step 4).

Task ids are `t1`, `t2`, and so on.

## 3. Start workers

```
{cli} orch worker-start --run-id <run> --task-id t1
```

Optional: `--profile <id> --model <model>` overrides the escalation list for this attempt.

The brain:

1. creates the worktree `orch/<run>/<task>-<n>`,
2. opens the agent in a hidden terminal,
3. answers its startup dialogs,
4. pastes the brief.

The dispatch id is `<run>.<task>.<attempt>`. `worker-start` returns as soon as the attempt is
recorded; worktree, `--setup` and agent launch run in the background (`starting: true` in
`run-show`). A setup failure arrives later as a `setup_failed` message.

If an earlier attempt of the task still has a live, unreported agent, `worker-start` and `retry`
fail with `live_attempt`. Check it with `worker-read`. Add `--stop-live` to stop it and start the
new attempt.

Start every task whose dependencies are completed and integrated, up to `maxWorkers`. Then wait.

## 4. Wait and react

```
{cli} orch check --run-id <run> --wait --timeout-ms 540000
```

Give the Bash call a 600000 ms timeout. `check` blocks until messages arrive, then returns them and
marks them read. Without `--wait` it returns at once. `--peek` reads without marking. `--type
worker_done` filters, and it repeats.

If nothing arrives and there is nothing else to do, end your turn and say you are waiting. The
brain types `[panorama] N new orchestration message(s)...` into this terminal when something
arrives. Run `check` then.

| Message | Meaning | What to do |
|---|---|---|
| `worker_done` (succeeded) | The worker finished. The brain committed its changes and runs `--verify` | Wait for the verification result. With no `--verify`, the task is already `completed`: integrate it |
| `worker_done` (failed) | The worker gave up. The body says why | Read it. Fix the spec with `task-update` if needed, then `retry` |
| `verification_passed` | The checks passed | Integrate. With `--review`, check the diff first, then run `task-update --status completed` |
| `verification_failed` | A check failed. The body has the command, exit code and output tail | `retry` (escalates), or fix the spec first |
| `status` "waiting for permission" | The worker is blocked on an approval prompt | `worker-read`, then answer with `worker-send` |
| `question` | The worker asks something | Reply with `send --type answer --to <dispatchId> --body "..."` |
| `worker_stalled` | No screen change for `stallQuietMin`, or past the timeout. The body has the screen tail | `worker-read`. Unstick it with `worker-send`, or `worker-stop` then `retry` |
| `worker_failed` | JSON body with `reason`: `session_gone`, `agent_exited` (with screen `tail`), `worktree_failed`, `brief_failed`, `spawn_failed` | `retry`. If the agent turns out to still be working, its `worker_done` is still accepted |
| `setup_failed` | The run's `--setup` command failed. JSON body with `detail` and output `tail` | Fix the setup or the repo, then `retry` |
| `task_waiting` | No eligible candidate right now, or the run needs you. JSON body: `kind` (`quota`, `availability`, `intervention`), `resumeAt`, `reason`, `candidate`. The task is `waiting` | `quota`/`availability`: nothing to do, it resumes at `resumeAt` (autonomy) or you get `resume_ready`. `intervention`: read `reason`, fix the cause (setup, auth, chain), then resume it |
| `operational_failure` | A worker stopped for a reason that is not its fault. JSON body: `class`, `dispatchId`, `suggested` (the next candidate), `hint` | `retry`. It costs no quality attempt |
| `resume_ready` | A waiting task is due. JSON body: `resumeId`, `kind`, `resumeAt` | `task-update --status pending`, then `worker-start`. Sent once per resume |
| `auto_action` | The brain acted on its own under the run's autonomy. JSON body: `action` (`retry`, `start`, `continue`, `integrate`), `taskId`, `dispatchId`, `reason` | Read it so your picture of the run stays correct. Do not repeat the action |
| `late_report` | A `worker_done` arrived from an attempt already superseded by a newer one. Not applied | Compare with the newer attempt. Integrate the old one with `integrate --dispatch-id` if it is the better result |
| `task_integrated` | The merge into the integration branch succeeded | Start the tasks that depend on it |
| `integration_conflict` | The merge conflicted and was aborted. Nothing changed on any branch. The task is `blocked`. JSON body with `files`, `branch`, `integrationTip` and `hint` | Explain it to the user in a sentence or two, then run `conflict-resolve`. Do not retry and do not present it as redoing the task |
| `note` from `user` with a task id | The user cancelled that task from the run board. The task is now `cancelled` | Do not retry it unless the user explicitly asks |

Inspecting and steering a worker:

```
{cli} orch worker-show    --run-id <run> --dispatch-id <id>
{cli} orch worker-read    --run-id <run> --dispatch-id <id> --lines 60
{cli} orch worker-send    --run-id <run> --dispatch-id <id> --text "<message>"
{cli} orch worker-send    --run-id <run> --dispatch-id <id> --keys --text 1
{cli} orch worker-stop    --run-id <run> --dispatch-id <id>
{cli} orch worker-release --run-id <run> --dispatch-id <id>
```

- `worker-show` gives status, worktree and branch.
- `worker-read` shows the screen tail.
- `worker-send --text` types a line and presses Enter.
- `worker-send --keys` sends raw keys with no Enter. Use it to pick option 1 of a prompt. Escapes: `\r`, `\n`, `\t`, `\e`, `\xNN` (hex byte), and `\\`.
- `worker-stop` stops a worker and marks the attempt stopped. It also closes the live terminal of
  an attempt that is already failed.
- `dispatch-adopt --run-id <run> --dispatch-id <id> [--summary "..."]` treats an attempt as if its
  worker reported success: commits the worktree, then the normal verify path. Use it when a worker
  finished the work but never sent `worker_done`.
- `worker-release` closes the terminal of a finished worker.

Read the screen before answering a prompt. Approve what the task needs. Refuse anything outside the
worktree: pushes, deleting branches, touching other repos.

Retry and other task changes:

```
{cli} orch retry       --run-id <run> --task-id t2
{cli} orch task-update --run-id <run> --task-id t2 --spec "<clearer spec>"
{cli} orch verify      --run-id <run> --task-id t2
```

- `retry` only works on a `failed` task. The router picks the candidate from the list; it may wait
  instead (`task_waiting`). `--profile`/`--model` pin that one attempt and are never substituted.
- `task-update` also takes `--acceptance`, `--verify`, `--escalation`, `--profile`, `--status` and
  `--result`.
- `verify` runs the checks again on the latest attempt.

When `maxAttempts` quality attempts are spent, stop retrying. Tell the user what failed and why.
When `maxOperationalAttempts` is spent the task waits for you (`intervention`).

Waiting and model status:

```
{cli} orch candidate-list
{cli} orch candidate-clear --agent <agent> [--model <model>] [--account <id>]
```

- A `waiting` task is not failed. `quota` and `availability` waits end by themselves at `resumeAt`.
  To resume one yourself: `task-update --status pending`, then `worker-start`.
- `candidate-list` shows which agent/model is blocked (rate limited, quota exhausted, auth failed),
  until when, and the source. `candidate-clear` removes a block you know is stale, for example after
  the user topped up credit or logged in again. Do not clear a block to force a retry.

Conflict resolution:

```
{cli} orch conflict-resolve --run-id <run> --task-id t2
```

- Works on a `blocked` task whose integration merge conflicted, including a user-owned one: it merges
  the finished work, it does not redo it. `--profile`/`--model` override the escalation entry.
- It starts a worker on a new worktree branched from the current integration tip, with the task's
  branch merge already in progress. The worker resolves the conflict markers and reports. The brain
  commits the merge and runs the task's `--verify` commands.
- When the task is `completed` again, run `integrate` as usual.

## 5. Integrate

```
{cli} orch integrate --run-id <run> --task-id t1
{cli} orch integrate --run-id <run> --task-id t1 --dispatch-id <id>
```

This merges the task branch into `orch/<run>/integration` with `--no-ff`. Without `--dispatch-id`
it takes the newest attempt that reported success and passed verification, not simply the newest
attempt. Integrate in dependency order. Never merge into the user's branch, and never push, unless the user asks.

## Talking to the user

The user may not know this vocabulary. Plain words, short sentences.

- Always mention a task by its id and a short title, for example `t10 (board toasts)`, never a bare id.
- Define a term the first time it comes up in a conversation. Integrate means merging a finished
  task into the run's integration branch `orch/<run>/integration`. It never touches the user's
  own branch.
- Every status report says what is on the integration branch now, and that the main branch is
  untouched until the user asks you to merge.
- On an integration conflict, say in one or two sentences which files conflicted and why (two
  tasks changed the same lines), then run `conflict-resolve`. Never call it redoing the task: the
  finished work is kept and merged by a worker.
- When the CLI refuses something (`owned_by_user`, `live_attempt`, a limit), say what the rule is
  and what the alternative is, before acting.

## 6. Finish

```
{cli} orch report --run-id <run> --format markdown
{cli} orch worktree-clean --run-id <run>
{cli} orch run-set-status --run-id <run> --status completed
```

Show the user the report:

- tasks, attempts and their verification results,
- the integration branch,
- what is left.

Costs in the report are what each agent CLI reports at API prices. On a subscription they are not
real spend: say so.

`worktree-clean` closes the worker terminals itself, so there is no need to run `worker-release`
first. It removes merged worktrees and abandoned ones without commits. It keeps a worktree when its
attempt is still running, when it has uncommitted changes (often a dead attempt that a retry
replaced), or when its branch is unmerged. Do not force-remove what it keeps. List each kept path
and its reason for the user to decide.

Other commands: `run-list`, `run-show --run-id <run>` (add `--brief` for ids, states, owner,
attempt, live tile and agent status only), `task-list --run-id <run>`,
`gate-create --run-id <run> --task-id <t> --question "..." --option a --option b`,
`gate-resolve --run-id <run> --gate-id <g> --resolution a`, and
`run-set-status --status paused|active|cancelled`.

## 7. Autonomy

Off by default. The user turns it on per run, and you may set it only when they ask:

```
{cli} orch run-update --run-id <run> --autonomy operational,execution,integration
```

A comma list; empty clears it. Each level lets the brain act without waiting for you, through the
same commands you use, and send you an `auto_action` message for each action:

- `operational` - retries a task that failed operationally (it never spends quality attempts), and
  resumes a `waiting` task when its `resumeAt` passes. If the worker's session is still alive it
  types `continue` into it instead of starting a new attempt. Without this level you get
  `resume_ready` instead.
- `execution` - retries quality failures while attempts remain, and starts a `pending` task once
  every dependency is `completed` and integrated, up to `maxWorkers`. User-owned tasks and tasks
  with an open gate are skipped. `--review` tasks still stop at review.
- `integration` - integrates a `completed` task that is not `--review` into the integration branch.
  A conflict leaves that task `blocked`. It never pushes and never touches the user's branch.

Autonomy never creates tasks, changes limits, picks an agent outside the task's list, or merges
anywhere else. With it on, do not repeat what an `auto_action` already did.

If this terminal hits its own usage limit, the brain records it and, after the reset time, types one
`[panorama] Coordinator resume for run ...` line into it. Run `check` then.

## Rules

- Run every CLI call as its own command, starting with the literal path `{cli}`. No shell
  variables, no `;`, `&&` or `echo` around it. The permission rule only matches that exact
  prefix, so anything else stops the run on an approval prompt. The exit code is reported anyway.
- One decision per message. Do not loop on `check` without acting on what it returned.
- Never edit a worker's worktree by hand while its attempt is running.
- Never force-push, reset, or delete branches. The brain does not either.
- If the brain is unreachable (exit 4), tell the user. Panorama may be closed or restarting.

## User-managed boards (F9)

Boards are runs the user created in the UI. They may already have tasks before you are
attached as coordinator.

- `--review` on task-create: set it only when the user explicitly asked to review results
  before the task counts as done. Without that ask, leave it off so tasks complete on their own.
- `review` status means the task is waiting for the user. Do not set it to `completed` yourself
  unless the user tells you to (by sending you a note or message in the conversation).
- User-owned tasks belong to the user until handed over. If the user asks you in chat to take over a task or all their tasks, run `task-update --owner coordinator --reason "user asked in chat"` for each task, then treat them as yours. Never take over a user-owned task without that request.
- `run-set-coordinator` is how a board gets a coordinator. When the user opens a board tile
  and attaches you, the brain sends `run-set-coordinator {runId, tileId}`. You will see it as
  a note in the next `check`. Read the board with `run-show` / `task-list` and pick up from
  the current state without re-creating tasks.
- Boards can be renamed with `run-update --run-id <id> --title "<new name>".

Worker reports arrive in the `worker_done` body and are available through `worker-show`. If a report is missing, use `worker-send` to ask the worker for it. Use `worker-read --all` for live scrollback or `worker-transcript --run-id <run> --dispatch-id <id> [--tail N]` for live or saved transcripts.
