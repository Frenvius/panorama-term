---
name: orchestrate
description: Coordinate a multi-agent run in Panorama - split work into tasks, start CLI agent workers (claude, codex, opencode, kimi, pi, antigravity) in their own git worktrees, react to their reports, verify, retry, integrate, and report. Use when the user asks to orchestrate, delegate, fan out, or parallelize work across worker agents, or to run tasks overnight with cheaper models.
allowed-tools: Bash({cli} orch:*), PowerShell({cli} orch:*)
---

# Orchestrating workers in Panorama

You are the coordinator. You plan, start workers, read their results, and decide what happens next.
The Panorama brain records every step in an event log, runs verification, and commits and merges
the work. It never starts a worker on its own: every dispatch is your decision.

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
`--stall-quiet-min 5`. Note the returned `run.id`.

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
- An escalation entry is `profile` or `profile:model`. Attempt N uses entry N, and the last entry
  repeats.
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
| `late_report` | A `worker_done` arrived from an attempt already superseded by a newer one. Not applied | Compare with the newer attempt. Integrate the old one with `integrate --dispatch-id` if it is the better result |
| `task_integrated` | The merge into the integration branch succeeded | Start the tasks that depend on it |
| `integration_conflict` | The merge conflicted and was aborted. The task is `blocked` | Lists the files. Re-scope the task against the new base and retry, or ask the user |
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
- `worker-send --keys` sends raw keys with no Enter. Use it to pick option 1 of a prompt.
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

- `retry` only works on a `failed` task. It starts the next attempt with the next escalation entry.
  `--profile`/`--model` override that entry.
- `task-update` also takes `--acceptance`, `--verify`, `--escalation`, `--profile`, `--status` and
  `--result`.
- `verify` runs the checks again on the latest attempt.

When `maxAttempts` is reached, stop retrying. Tell the user what failed and why.

## 5. Integrate

```
{cli} orch integrate --run-id <run> --task-id t1
{cli} orch integrate --run-id <run> --task-id t1 --dispatch-id <id>
```

This merges the task branch into `orch/<run>/integration` with `--no-ff`. Without `--dispatch-id`
it takes the newest attempt that reported success and passed verification, not simply the newest
attempt. Integrate in dependency order. Never merge into the user's branch, and never push, unless the user asks.

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
- owner `user` on a task means the user will start and retry that task themselves. Do not
  dispatch it. Do not retry it. If you receive a worker_done or failure for a user-owned task,
  report it to the user and wait for their instruction.
- `run-set-coordinator` is how a board gets a coordinator. When the user opens a board tile
  and attaches you, the brain sends `run-set-coordinator {runId, tileId}`. You will see it as
  a note in the next `check`. Read the board with `run-show` / `task-list` and pick up from
  the current state without re-creating tasks.
- Boards can be renamed with `run-update --run-id <id> --title "<new name>".
