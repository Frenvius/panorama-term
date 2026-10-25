import type { EngineInterface, Register } from 'claude-code'

const DESC_MAX = 160
const EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max']
const COMMAND_DIALOG_MS = 500

let lastTurns = 0
let contextStale = false
const askedTools = new Set<string>()
let toolDialog = false
let commandDialog = false
let elicitDialog = false

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

async function settleTool($: EngineInterface, id: string | undefined) {
  if (!id || !askedTools.delete(id) || !toolDialog || askedTools.size) return
  toolDialog = false
  await post($, { status: 'busy', dialog: dialogOpen() })
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
    await reportStatus($, { dialog: false, effort: await configuredEffort($) })
    return started
  })

  on('command.run', { command: 'tui' }, async ($, e, next) => {
    if (!(await $.env.get('PANORAMA_TILE_ID'))) return next(e)
    return { text: 'Panorama keeps Claude Code on the fullscreen renderer.' }
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

  on('turn.start', async ($, e, next) => {
    if (e.agentId !== undefined) return next(e)
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
    const result = yield* next(e)
    if (e.agentId !== undefined) return result
    contextStale = false
    await reportStatus($, typeof e.effort === 'string' ? { effort: e.effort } : {})
    return result
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    await syncTurns($)
    if (e.agentId !== undefined) return done
    askedTools.clear()
    toolDialog = false
    commandDialog = false
    await post($, { status: 'idle', dialog: dialogOpen() })
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
    await forwardHook($, 'permission', e)
    return next(e)
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
    const result = await next(e)
    await settleTool($, e.tool_use_id)
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

  on('classic.Stop', async ($, e, next) => {
    await post($, { permissionMode: e.permission_mode, effort: e.effort?.level })
    if (!e.stop_hook_active) await forwardHook($, 'stop', e)
    return next(e)
  })
}
