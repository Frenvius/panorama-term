---
name: orchestrate
description: Coordinate independent or long mechanical tasks with Panorama workers in separate worktrees.
allowed-tools: Bash({cli} orch:*), PowerShell({cli} orch:*)
---

# Orchestrating workers in Panorama

Orchestrate several independent tasks or long mechanical work. A run has a fixed coordinator cost;
do small or quick single tasks yourself. Agree on the plan with the user, including the repository,
base ref, task boundaries, ordering, roles and verification. Each worker sees its brief, not the
conversation. Give each task a self-contained spec, acceptance criteria and scoped checks; use
different files for parallel tasks where possible. The brain chooses agents by role unless the user
requests an override. Add dependent test or review tasks when judgment is needed.

## Plan and start

Write a `plan.json` file and run `plan-apply` once. The JSON has a `run` object, a `tasks` array,
and optional `start` boolean. Each task needs `title` and `spec`; optional fields include `key`,
`role`, `acceptance`, `verify` (array of shell commands), and `deps` (array of task keys or ids).
Roles are `explore`, `implement`, `test`, `review`, and `mechanical`. Example:

```json
{
  "run": {"repo": "/absolute/path/to/repo", "title": "Update components"},
  "tasks": [
    {"key": "one", "title": "Update component", "spec": "Update component A and its tests.", "role": "implement", "verify": ["npm test"]},
    {"title": "Review update", "spec": "Review component A for regressions.", "role": "review", "deps": ["one"]}
  ]
}
```

```
{cli} orch plan-apply --file <absolute-path-to-plan.json>
```

The command validates and creates the run and tasks together, assigns `t1`, `t2`, etc. in array
order, and starts ready root tasks up to the worker limit. Inspect the returned run id, task ids and
started dispatches. Detached mode defaults on, with operational, execution and integration autonomy;
the brain starts dependents, retries and integrates as eligible. Do not start roots again. Tell the
user the plan is running and that you are waiting, then end the turn. Do not poll.

## On a decision wake

When a `[panorama] ... decision(s) needed` notice arrives, run:

```
{cli} orch check --run-id <run> --decisions
```

Handle each decision, then end the turn. Routine messages are acknowledged and summarized in the
`routine` digest; `--peek` does not acknowledge them. Detached worker questions receive an automatic
answer to use the spec and best judgment and explain the decision in the report. The brain handles
eligible stalls and conflicts under autonomy. If autonomy is disabled or cannot handle them, decide
what to do using the reference.

| Decision | Action |
|---|---|
| `setup_failed` | Inspect the failure, fix the setup or repo, then retry. |
| `gate` | Inspect with `run-show`, ask the user if needed, and `gate-resolve`. |
| User `note` | Follow it; do not retry a cancelled task unless the user asks. |
| `worker_stalled` or `integration_conflict` | Inspect and resolve or ask the user. Use `conflict-resolve` for a merge conflict, not a task retry. |
| `run_idle` | Read the grouped digest and decide for each task: retry, resume, start, integrate, resolve a gate, cancel, or ask the user. |
| `run_finished` | Finish as below. |

## Finish

Show the user the digest and report: tasks, attempts, checks, integration branch and remaining work.
Costs reported at API prices are not actual subscription spend. Run:

```
{cli} orch report --run-id <run> --format markdown
{cli} orch worktree-clean --run-id <run>
{cli} orch run-set-status --run-id <run> --status completed
```

Cleanup closes worker terminals and retains running, dirty or unmerged worktrees. List retained paths
and reasons for the user; do not force-remove them.

## Rules

- Start every CLI call with the literal path `{cli}` and run it as its own command. No shell
  variables, wrappers, command chaining or `echo`; permission rules match the literal prefix.
- Never push or merge into the user's branch. Integration merges only into the run's integration branch.
- Use plain words. Name each task by id and title, e.g. `t1 (update component)`. Explain unfamiliar
  terms and say what is on the integration branch when reporting status.
- Do not edit a running worker's worktree, force-push, reset, or delete branches. If the brain is
  unreachable, tell the user.

For attached mode, all message types, steering, retries, conflicts, boards and autonomy details, read
`reference.md` in this skill directory when needed.

## When a worktree cannot do the task

A worker runs in an isolated git worktree. If the task needs gitignored files, a heavy build
artifact, or a running app that is not in the worktree, the worktree cannot serve it.

Most of these still run in parallel worktrees. Prefer, in order:

- Share what the build only reads: `link` it (`readOnly` when workers must not write it), or
  rely on the tool's global cache. Never `link` a directory the build writes; `copy` it or
  let each worktree produce its own.
- Isolate run-time state per worker: port, database, data directory, queue prefix. The spec
  tells the worker to take them from env or a task-specific name.
- Split implement (parallel, no lock) from build/validate or run-app tasks, and put an
  `exclusive` lock only on the part that cannot be isolated.
- Use `workspace: "main"` only when none of the above works. Two in-place tasks never run
  together: they would build and verify each other's half-done edits.

When the route is not clear from the repo: stop and ask the user which route to take (the
options above, an in-place task, or doing it by hand), and which model to use. Never bypass
with the Agent tool, subagents, or another agent CLI. Do not override the profile or model
unless the user asks.

## One task: `orch exec`

For a single self-contained task, skip the conversation loop:

```
{cli} orch exec --file <plan.json> [--timeout-min N]
```

Applies the plan (same JSON shape as `plan-apply`), waits until done or a decision is needed,
prints a compact markdown report, and exits. Exit codes: 0 all done, 1 a task failed, 2
decision needed (run `{cli} orch check --run-id <run> --decisions`), 3 timeout (default 240
min), 4 brain unreachable. Does not set `coordinatorTile`; nobody is woken during the wait.

## `.panorama/worktree.json`, `exclusive`, in-place tasks

Place `.panorama/worktree.json` in the repo root (may be gitignored). The brain reads it at
every dispatch start. `link` junctions/symlinks directories into each worktree. `copy` copies
files or directories. `exclusive` lists lock names tasks can hold (task field
`exclusive: ["name"]`; name must be in the list). The reserved name `main` is implicit for
in-place tasks.

`workspace: "main"` on a task runs the worker in the main checkout instead of a worktree.
Requires `run.allowInPlace: true` on the run. One in-place task at a time per repo. The worker
must not commit, switch branches, reset or stash. The diff stays in the working tree.

See `reference.md` for the full field reference.
