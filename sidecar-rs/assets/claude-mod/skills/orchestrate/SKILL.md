---
name: orchestrate
description: Coordinate a multi-agent run in Panorama - split work into tasks, start CLI agent workers (claude, codex, opencode, kimi, pi) in their own git worktrees, react to their reports, verify, retry, integrate, and report. Use when the user asks to orchestrate, delegate, fan out, or parallelize work across worker agents, or to run tasks overnight with cheaper models.
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

Agree on these before creating anything:

- **Repo** and base ref (default `HEAD`). Workers branch from it.
- **Tasks**. Each one is a self-contained unit of work a cheaper model can finish alone.
- **Workers**. Use the profiles and models the user names. If they named none, propose `claude`
  with model `haiku` and escalation to `claude:sonnet`. Built-in profiles: `claude` (models
  `haiku`, `sonnet`, `opus`), `codex`, `opencode` (model is `provider/model`), `kimi`, `pi`.
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
{cli} orch run-create --repo <abs path> --title "<title>" --coordinator-tile "$PANORAMA_TILE_ID"
```

Optional: `--base-ref <ref>`, `--setup "<command run in each new worktree>"` (e.g. `bun install`),
`--max-workers 4`, `--max-attempts 3`, `--dispatch-timeout-min 60`, `--verify-timeout-min 15`,
`--stall-quiet-min 5`. Note the returned `run.id`.

`--coordinator-tile` lets the brain type a short notice into this terminal when messages arrive
while you are idle. Keep it.

```
{cli} orch task-create --run-id <run> --title "<short title>" --spec "<full spec>" \
  --acceptance "<what done means>" --verify "<command>" --dep t1 \
  --profile claude --escalation claude:haiku --escalation claude:sonnet
```

- `--spec-file <path>` reads a long spec from a file.
- `--verify`, `--dep` and `--escalation` repeat.
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

The dispatch id is `<run>.<task>.<attempt>`.

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
| `worker_failed` | The agent process exited, or its terminal is gone | `retry` |
| `task_integrated` | The merge into the integration branch succeeded | Start the tasks that depend on it |
| `integration_conflict` | The merge conflicted and was aborted. The task is `blocked` | Lists the files. Re-scope the task against the new base and retry, or ask the user |

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
- `worker-stop` stops a worker and marks the attempt stopped.
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
```

This merges the task branch into `orch/<run>/integration` with `--no-ff`. Integrate in dependency
order. Never merge into the user's branch, and never push, unless the user asks.

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

Other commands: `run-list`, `run-show --run-id <run>`, `task-list --run-id <run>`,
`gate-create --run-id <run> --task-id <t> --question "..." --option a --option b`,
`gate-resolve --run-id <run> --gate-id <g> --resolution a`, and
`run-set-status --status paused|active|cancelled`.

## Rules

- One decision per message. Do not loop on `check` without acting on what it returned.
- Never edit a worker's worktree by hand while its attempt is running.
- Never force-push, reset, or delete branches. The brain does not either.
- If the brain is unreachable (exit 4), tell the user. Panorama may be closed or restarting.
