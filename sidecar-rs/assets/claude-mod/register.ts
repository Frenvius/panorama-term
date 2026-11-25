import type { AgentInfo, BackgroundTaskSummary, EngineInterface, PermissionRequestDecision, Register, TurnStepChunk } from 'claude-code'

const DESC_MAX = 160
const EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max']
const COMMAND_DIALOG_MS = 500
const LIVE_FLUSH_MS = 100
const LIVE_TEXT_MAX = 16000
const AGENT_SETTLE_MS = 1500
const AGENT_RECHECK_MS = 300
const ACTIVE_AGENT = new Set(['pending', 'running', 'waiting'])
const VIEW_COMMAND = 'panorama-view'
const SHELL_OUTPUT_RE = /Output is being written to: (.+?\.output)/

type LivePhase = 'idle' | 'thinking' | 'text' | 'tool'
type LiveState = { seq: number; turnId: string; phase: LivePhase; text: string; tool?: string }

let live: LiveState = { seq: 0, turnId: '', phase: 'idle', text: '' }
let liveBlock = -1
let liveFlushPending = false

let lastTurns = 0
let contextStale = false
const askedTools = new Set<string>()
let toolDialog = false
let commandDialog = false
let elicitDialog = false
let mainTurn = false
const subagentStart = new Map<string, number>()
const subagentTool = new Map<string, string>()
// What each running subagent did last (tool and its main argument), newest last, for the tasks tile.
const ACTIVITY_MAX = 6
const subagentActivity = new Map<string, string[]>()
// How tracked subagents ended (completed, failed, killed), newest last.
const agentEnds = new Map<string, string>()
const AGENT_ENDS = new Set(['completed', 'failed', 'killed'])
let lastSubagents = '[]'
let lastView: string | undefined

type ShellRow = { id: string; command: string; description?: string; outputFile?: string; startedAt: number; monitor?: true }
const shells = new Map<string, ShellRow>()
let lastShells = '[]'
// How tracked shells ended (completed, failed, killed), newest last; the app shows it on finished tasks.
const SHELL_ENDS_MAX = 50
const shellIds = new Set<string>()
const shellEnds = new Map<string, string>()
// Other agent CLIs the model runs in the foreground (`codex exec ...`), by tool_use_id; listed with the shells.
const CLI_RE = /(?:^|[\s;&|(/])(codex|gemini|agy|antigravity|opencode|kimi|cursor-agent|aider)(?=\s|$)/
const calls = new Map<string, ShellRow>()

const dialogOpen = () => toolDialog || commandDialog || elicitDialog

type ToolRow = { tool_use_id?: string; isRunning: boolean; isErrored: boolean; isInterrupted: boolean; output?: unknown }

const isSettled = (row: ToolRow) => row.isRunning || row.isErrored || row.isInterrupted || row.output !== undefined

async function syncTurns($: EngineInterface) {
  lastTurns = await $.session.turns()
}

async function sidecarUrl($: EngineInterface, path: string) {
  const tileId = await $.env.get('PANORAMA_TILE_ID')
  if (!tileId) return undefined
  const port = (await $.env.get('PANORAMA_SIDECAR_PORT')) ?? '9777'
  const sep = path.includes('?') ? '&' : '?'
  return `http://127.0.0.1:${port}${path}${sep}tileId=${encodeURIComponent(tileId)}`
}

async function post($: EngineInterface, state: Record<string, unknown>) {
  const tileId = await $.env.get('PANORAMA_TILE_ID')
  if (!tileId) return
  const port = (await $.env.get('PANORAMA_SIDECAR_PORT')) ?? '9777'
  try {
    await $.http.fetch(`http://127.0.0.1:${port}/agent-status`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ source: 'mod', agent: 'claude', tileId, sessionId: await $.session.id(), ...state }),
    })
  } catch (err) {
    $.ui.log(`panorama: post failed: ${String(err)}`, { to: 'debug' })
  }
}

async function forwardHook($: EngineInterface, event: string, input: unknown) {
  const url = await sidecarUrl($, `/agent-hook?event=${event}`)
  if (!url) return ''
  try {
    const res = await $.http.fetch(url, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    })
    return res.ok ? res.text : ''
  } catch (err) {
    $.ui.log(`panorama: ${event} hook failed: ${String(err)}`, { to: 'debug' })
    return ''
  }
}

async function remoteDecision($: EngineInterface, promptId: string) {
  const url = await sidecarUrl($, `/agent-answer?promptId=${encodeURIComponent(promptId)}`)
  if (!url) return undefined
  try {
    for (;;) {
      const res = await $.http.fetch(url)
      if (!res.ok || !res.text) return undefined
      if (res.text !== 'pending') return JSON.parse(res.text) as PermissionRequestDecision
    }
  } catch (err) {
    $.ui.log(`panorama: remote answer failed: ${String(err)}`, { to: 'debug' })
    return undefined
  }
}

async function sendLive($: EngineInterface) {
  const tileId = await $.env.get('PANORAMA_TILE_ID')
  if (!tileId) return
  const port = (await $.env.get('PANORAMA_SIDECAR_PORT')) ?? '9777'
  const text = live.text.length > LIVE_TEXT_MAX ? live.text.slice(-LIVE_TEXT_MAX) : live.text
  try {
    await $.http.fetch(`http://127.0.0.1:${port}/agent-live`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ tileId, ...live, text }),
    })
  } catch (err) {
    $.ui.log(`panorama: live post failed: ${String(err)}`, { to: 'debug' })
  }
}

function scheduleLive($: EngineInterface) {
  if (liveFlushPending) return
  liveFlushPending = true
  void $.clock.after(LIVE_FLUSH_MS, () => {
    liveFlushPending = false
    void sendLive($)
  })
}

function setLivePhase($: EngineInterface, turnId: string, phase: LivePhase, tool?: string) {
  live = { seq: live.seq + 1, turnId, phase, text: '', tool }
  void sendLive($)
}

function trackLive($: EngineInterface, turnId: string, chunk: TurnStepChunk) {
  if (chunk.kind !== 'text' && chunk.kind !== 'thinking' && chunk.kind !== 'tool') return
  if (chunk.index !== liveBlock || chunk.kind !== live.phase) {
    liveBlock = chunk.index
    setLivePhase($, turnId, chunk.kind, chunk.kind === 'tool' ? chunk.name : undefined)
  }
  if (chunk.kind === 'tool' || !chunk.text) return
  live = { ...live, seq: live.seq + 1, text: live.text + chunk.text }
  scheduleLive($)
}

async function settleTool($: EngineInterface, id: string | undefined) {
  if (!id || !askedTools.delete(id) || !toolDialog || askedTools.size) return
  toolDialog = false
  await post($, { status: 'busy', dialog: dialogOpen() })
}

async function listAgents($: EngineInterface): Promise<AgentInfo[]> {
  try {
    return await $.agent.list()
  } catch (err) {
    $.ui.log(`panorama: agent list failed: ${String(err)}`, { to: 'debug' })
    return []
  }
}

async function agentsActive($: EngineInterface, except?: string) {
  return (await listAgents($)).some(a => a.id !== except && ACTIVE_AGENT.has(a.status))
}

function recordEnd(ends: Map<string, string>, id: string, status: string) {
  if (ends.get(id) === status) return
  ends.delete(id)
  ends.set(id, status)
  if (ends.size > SHELL_ENDS_MAX) ends.delete(ends.keys().next().value!)
}

async function reportSubagents($: EngineInterface) {
  const now = Date.now()
  const all = await listAgents($)
  const agents = all.filter(a => ACTIVE_AGENT.has(a.status))
  for (const a of agents) if (!subagentStart.has(a.id)) subagentStart.set(a.id, now)
  for (const id of [...subagentStart.keys()]) {
    if (agents.some(a => a.id === id)) continue
    const status = all.find(a => a.id === id)?.taskStatus
    recordEnd(agentEnds, id, status && AGENT_ENDS.has(status) ? status : 'completed')
    subagentStart.delete(id)
    subagentTool.delete(id)
    subagentActivity.delete(id)
  }
  const rows = agents.map(a => ({
    id: a.id,
    type: a.type,
    description: a.description,
    status: a.status,
    tool: subagentTool.get(a.id),
    startedAt: subagentStart.get(a.id),
    label: a.label && a.label !== a.description ? a.label.slice(0, DESC_MAX * 4) : undefined,
    model: a.model,
    tokens: a.tokenCount,
    activity: subagentActivity.get(a.id),
  }))
  const ends = Object.fromEntries(agentEnds)
  const key = JSON.stringify([rows, ends])
  if (key === lastSubagents) return
  lastSubagents = key
  await post($, { subagents: rows, agentEnds: ends })
}

const SUMMARY_KEYS = ['command', 'file_path', 'path', 'pattern', 'url', 'query', 'description', 'prompt']

// One line for what a tool call does: the tool and its main argument.
function toolSummary(input: Record<string, unknown>) {
  const arg = SUMMARY_KEYS.map(k => input[k]).find(v => typeof v === 'string' && v)
  const text = String(input.tool) + (typeof arg === 'string' ? `: ${arg.split('\n')[0]}` : '')
  return text.length > DESC_MAX ? `${text.slice(0, DESC_MAX - 1)}…` : text
}

function pushActivity(agentId: string, line: string) {
  const list = [...(subagentActivity.get(agentId) ?? []), line].slice(-ACTIVITY_MAX)
  subagentActivity.set(agentId, list)
}

function trackCall(input: Record<string, unknown>) {
  const id = input.tool_use_id
  const command = input.command
  if (input.tool !== 'Bash' || input.run_in_background || typeof id !== 'string' || typeof command !== 'string') return undefined
  if (!CLI_RE.test(command)) return undefined
  shellIds.add(id)
  calls.set(id, {
    id,
    command,
    description: typeof input.description === 'string' ? input.description : undefined,
    startedAt: Date.now(),
  })
  return id
}

async function reportShells($: EngineInterface) {
  const rows = [...shells.values(), ...calls.values()]
  const ends = Object.fromEntries(shellEnds)
  const key = JSON.stringify([rows, ends])
  if (key === lastShells) return
  lastShells = key
  await post($, { shells: rows, shellEnds: ends })
}

function recordShellEnd(id: string, status: string) {
  if (!shellIds.has(id) || shellEnds.get(id) === status) return false
  recordEnd(shellEnds, id, status)
  return true
}

// A monitor's result names no file; its output sits beside the shells' in the session's tasks folder.
function siblingOutput(id: string) {
  const known = [...shells.values()].find(s => s.outputFile)?.outputFile
  return known ? `${known.slice(0, known.lastIndexOf('/') + 1)}${id}.output` : undefined
}

function trackShell(
  e: { tool: unknown; command?: unknown; description?: unknown; ws?: { url?: unknown } },
  result: { result?: unknown; text?: string },
) {
  if (e.tool !== 'Bash' && e.tool !== 'Monitor') return false
  const record = result.result as { backgroundTaskId?: unknown; taskId?: unknown } | undefined
  const id = e.tool === 'Bash' ? record?.backgroundTaskId : record?.taskId
  if (typeof id !== 'string' || shells.has(id)) return false
  shellIds.add(id)
  shells.set(id, {
    id,
    command: String(e.command ?? e.ws?.url ?? ''),
    description: typeof e.description === 'string' ? e.description : undefined,
    outputFile: result.text?.match(SHELL_OUTPUT_RE)?.[1] ?? (e.tool === 'Monitor' ? siblingOutput(id) : undefined),
    startedAt: Date.now(),
    monitor: e.tool === 'Monitor' || undefined,
  })
  return true
}

async function dropEndedShells($: EngineInterface, inFlight: BackgroundTaskSummary[] | undefined) {
  if (!inFlight) return
  const live = new Set(inFlight.filter(t => t.type === 'shell' || t.type === 'monitor').map(t => t.id))
  for (const id of [...shells.keys()]) if (!live.has(id)) shells.delete(id)
  await reportShells($)
}

async function settleAgents($: EngineInterface, finished: string) {
  await reportSubagents($)
  if (mainTurn || (await agentsActive($, finished))) return
  await post($, { status: 'idle', dialog: dialogOpen() })
}

async function closeCommandDialog($: EngineInterface) {
  if (!commandDialog) return
  commandDialog = false
  await post($, { dialog: dialogOpen() })
}

async function reportStatus($: EngineInterface, extra: Record<string, unknown> = {}) {
  const { context, cost, rateLimits } = await $.session.usage()
  const rate = (kind: string) => rateLimits.find(r => r.kind === kind)?.percentUsed
  const fill = contextStale ? {} : { contextTokens: context.tokens, contextPercent: context.percent }
  await post($, {
    model: await $.session.model(),
    ...fill,
    contextWindow: context.window,
    costUsd: cost?.usd,
    rateFiveHour: rate('five_hour'),
    rateSevenDay: rate('seven_day'),
    ...extra,
  })
}

async function configuredEffort($: EngineInterface) {
  const model = await $.session.model()
  const perModel = (await $.settings.read({ key: 'modelSettings' })) as Record<string, { effortLevel?: string }> | undefined
  const level = perModel?.[model]?.effortLevel ?? ((await $.settings.read({ key: 'effortLevel' })) as string | undefined)
  return level && EFFORT_LEVELS.includes(level) ? level : undefined
}

async function reportSessionReset($: EngineInterface, source: string, resumedTokens: number | undefined) {
  contextStale = true
  const tokens = source === 'resume' || source === 'fork' ? resumedTokens : 0
  const { context } = await $.session.usage()
  const fill =
    tokens === undefined
      ? {}
      : { contextTokens: tokens, contextPercent: context.window ? Math.round((tokens / context.window) * 100) : 0 }
  await post($, { sessionReset: source, ...fill })
}

type CatalogEntry = {
  id: string
  name: string
  section?: string
  fast_mode?: unknown
  notice?: { text?: string | null }
  runtime?: { max_input_tokens?: number; effort_levels?: string[]; default_effort?: string }
}

async function readModelCatalog($: EngineInterface) {
  const home = (await $.env.get('CLAUDE_CONFIG_DIR')) ?? `${(await $.env.get('USERPROFILE')) ?? (await $.env.get('HOME'))}/.claude`
  const dir = `${home}/cache/model-catalog`
  try {
    const newest = (await $.fs.list(dir))
      .filter(f => f.name.endsWith('-cc.json'))
      .sort((a, b) => b.mtimeMs - a.mtimeMs)[0]
    if (!newest) return undefined
    const doc = JSON.parse(String(await $.fs.read(`${dir}/${newest.name}`)))
    const models: CatalogEntry[] = doc?.catalog?.config?.models ?? []
    return models.map(m => ({
      id: m.id,
      name: m.name,
      section: m.section === 'main' ? 'main' : 'more',
      contextWindow: m.runtime?.max_input_tokens,
      efforts: m.runtime?.effort_levels ?? [],
      defaultEffort: m.runtime?.default_effort,
      fast: Boolean(m.fast_mode),
      notice: m.notice?.text ?? undefined,
    }))
  } catch (err) {
    $.ui.log(`panorama: model catalog unreadable: ${String(err)}`, { to: 'debug' })
    return undefined
  }
}

async function reportCatalog($: EngineInterface) {
  const commands = (await $.command.list()).map(c => ({
    name: `/${c.name}`,
    desc: (c.description ?? '').slice(0, DESC_MAX),
    source: c.source,
  }))
  const config = await $.config.list()
  const row = (key: string) => config.find(r => r.key === key)
  await post($, {
    commands,
    commandArgs: { '/output-style': row('outputStyle')?.options ?? [] },
    modelOptions: row('model')?.options,
    modelCatalog: await readModelCatalog($),
    thinking: row('thinking')?.value,
    outputStyle: row('outputStyle')?.value,
  })
}

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await syncTurns($)
    await reportCatalog($)
    lastSubagents = '[]'
    lastView = ''
    shells.clear()
    shellIds.clear()
    shellEnds.clear()
    calls.clear()
    subagentActivity.clear()
    agentEnds.clear()
    lastShells = '[]'
    if (await $.env.get('PANORAMA_TILE_ID')) {
      await $.command.register({
        name: VIEW_COMMAND,
        description: "Show a subagent's transcript, or the main one without an id",
        argumentHint: '[agentId]',
        immediate: true,
      })
    }
    await reportStatus($, { status: 'idle', dialog: false, subagents: [], shells: [], viewing: '', effort: await configuredEffort($) })
    return started
  })

  on('command.run', { command: 'tui' }, async ($, e, next) => {
    if (!(await $.env.get('PANORAMA_TILE_ID'))) return next(e)
    return { text: 'Panorama keeps Claude Code on the fullscreen renderer.' }
  })

  on('command.run', { command: VIEW_COMMAND }, async ($, e) => {
    const agentId = e.args.trim()
    const shown = await $.ui.view(agentId ? { agentId } : {})
    if (shown.deny) return { text: `Panorama: ${shown.deny}` }
    lastView = agentId
    await post($, { viewing: agentId })
    return {}
  })

  on('ui.render', { component: 'AbovePrompt' }, ($, e, next) => {
    const viewing = e.props.view.agentId ?? ''
    if (viewing !== lastView) {
      lastView = viewing
      void post($, { viewing })
    }
    return next(e)
  })

  on('ui.render', { component: 'UserMessage' }, ($, e, next) => {
    const { id, status } = e.props.task ?? {}
    const ended = Boolean(id && status && recordShellEnd(id, status))
    if (id && (shells.delete(id) || ended)) void reportShells($)
    return next(e)
  })

  on('command.run', async ($, e, next) => {
    if (e.origin.kind !== 'plugin') {
      commandDialog = true
      void $.clock.after(COMMAND_DIALOG_MS, () => {
        if (commandDialog) void post($, { dialog: true })
      })
    }
    const result = await next(e)
    await syncTurns($)
    const level = e.args.trim().toLowerCase()
    if (e.command === 'effort' && EFFORT_LEVELS.includes(level)) await post($, { effort: level })
    await reportCatalog($)
    await reportStatus($)
    return result
  })

  on('agent.spawn', async ($, e, next) => {
    const spawned = await next(e)
    await reportSubagents($)
    return spawned
  })

  on('turn.start', async ($, e, next) => {
    if (e.agentId !== undefined) {
      void reportSubagents($)
      return next(e)
    }
    mainTurn = true
    commandDialog = false
    await post($, { status: 'busy', suggestion: '', dialog: dialogOpen() })
    return next(e)
  })

  on('session.compact', async ($, e, next) => {
    await closeCommandDialog($)
    return next(e)
  })

  on('ui.render', { component: 'PromptHint' }, ($, e, next) => {
    if (commandDialog && !e.props.isWorking) void closeCommandDialog($)
    return next(e)
  })

  on('ui.render', { component: 'ToolUse' }, ($, e, next) => {
    if (askedTools.size && isSettled(e.props)) void settleTool($, e.props.tool_use_id)
    return next(e)
  })

  on('ui.render', { component: 'ToolGroup' }, ($, e, next) => {
    if (askedTools.size) for (const call of e.props.calls) if (isSettled(call)) void settleTool($, call.tool_use_id)
    return next(e)
  })

  on('tool.check', async ($, e, next) => {
    const verdict = await next(e)
    if (verdict.decision === 'ask' && e.tool_use_id) askedTools.add(e.tool_use_id)
    return verdict
  })

  on('prompt.suggest', async ($, e, next) => {
    const result = await next(e)
    if (result.isShown) await post($, { suggestion: e.text })
    return result
  })

  on('turn.step', async function* ($, e, next) {
    if (e.agentId !== undefined) return yield* next(e)
    const stream = next(e)
    try {
      for await (const chunk of stream) {
        trackLive($, e.turnId, chunk)
        yield chunk
      }
    } finally {
      liveBlock = -1
      setLivePhase($, e.turnId, 'idle')
    }
    const result = await stream.result
    contextStale = false
    await reportStatus($, typeof e.effort === 'string' ? { effort: e.effort } : {})
    return result
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    await syncTurns($)
    if (e.agentId !== undefined) {
      const finished = e.agentId
      void reportSubagents($)
      void $.clock.after(AGENT_RECHECK_MS, () => void reportSubagents($))
      void $.clock.after(AGENT_SETTLE_MS, () => void settleAgents($, finished))
      return done
    }
    mainTurn = false
    askedTools.clear()
    toolDialog = false
    commandDialog = false
    await reportSubagents($)
    await post($, { status: (await agentsActive($)) ? 'busy' : 'idle', dialog: dialogOpen() })
    await reportStatus($)
    return done
  })

  on('classic.SessionStart', async ($, e, next) => {
    await forwardHook($, 'session-start', e)
    await reportSessionReset($, e.source, e.context_tokens)
    return next(e)
  })

  on('classic.PostModelSwitch', async ($, e, next) => {
    await reportStatus($, { model: e.to_model, effort: await configuredEffort($) })
    return next(e)
  })

  on('classic.PermissionRequest', async ($, e, next) => {
    toolDialog = true
    await post($, { status: 'waiting', dialog: true })
    const result = await next(e)
    if (result.decision) return result
    const promptId = await forwardHook($, 'permission', e)
    const decision = promptId ? await remoteDecision($, promptId) : undefined
    return decision ? { ...result, decision } : result
  })

  on('classic.Elicitation', async ($, e, next) => {
    elicitDialog = true
    await post($, { status: 'waiting', dialog: true })
    return next(e)
  })

  on('classic.ElicitationResult', async ($, e, next) => {
    elicitDialog = false
    await post($, { status: 'busy', dialog: dialogOpen() })
    return next(e)
  })

  on('classic.Notification', async ($, e, next) => {
    if (e.notification_type === 'idle_prompt') await forwardHook($, 'notification', e)
    return next(e)
  })

  on('tool.call', async ($, e, next) => {
    const input = e as unknown as Record<string, unknown>
    if (e.agentId !== undefined) {
      subagentTool.set(e.agentId, String(e.tool))
      pushActivity(e.agentId, toolSummary(input))
      void reportSubagents($)
    }
    const call = trackCall(input)
    if (call) await reportShells($)
    const result = await next(e)
    await settleTool($, e.tool_use_id)
    if (call) {
      calls.delete(call)
      recordShellEnd(call, 'deny' in result && result.deny ? 'killed' : 'isError' in result && result.isError ? 'failed' : 'completed')
      await reportShells($)
    }
    if (!('deny' in result && result.deny) && trackShell(e as Parameters<typeof trackShell>[0], result)) await reportShells($)
    return result
  })

  on('classic.UserPromptSubmit', async ($, e, next) => {
    await post($, { permissionMode: e.permission_mode })
    const context = await forwardHook($, 'prompt-submit', e)
    const result = await next(e)
    if (!context) return result
    return { ...result, additionalContext: [...(result.additionalContext ?? []), context] }
  })

  on('prompt.change', async ($, e, next) => {
    const turns = await $.session.turns()
    const rewound = turns < lastTurns
    lastTurns = turns
    if (e.origin.kind !== 'engine' || !rewound || !e.text.trim()) return next(e)
    const delivered = await forwardHook($, 'prompt-restore', { text: e.text })
    if (delivered === 'restored') void $.prompt.fill({ text: '', mode: 'replace' })
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    if (e.reason !== 'clear' && e.reason !== 'resume') await post($, { status: 'exited', subagents: [], shells: [] })
    return next(e)
  })

  on('classic.Stop', async ($, e, next) => {
    await post($, { permissionMode: e.permission_mode, effort: e.effort?.level })
    await dropEndedShells($, e.background_tasks)
    if (!e.stop_hook_active) await forwardHook($, 'stop', e)
    return next(e)
  })

  on('classic.SubagentStop', async ($, e, next) => {
    await dropEndedShells($, e.background_tasks)
    return next(e)
  })
}
