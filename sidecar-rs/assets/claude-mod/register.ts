import type { EngineInterface, Register } from 'claude-code'

const DESC_MAX = 160
const EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max']

let waiting = false

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

async function reportStatus($: EngineInterface) {
  const { context, cost, rateLimits } = await $.session.usage()
  const rate = (kind: string) => rateLimits.find(r => r.kind === kind)?.percentUsed
  await post($, {
    model: await $.session.model(),
    contextTokens: context.tokens,
    contextPercent: context.percent,
    contextWindow: context.window,
    costUsd: cost?.usd,
    rateFiveHour: rate('five_hour'),
    rateSevenDay: rate('seven_day'),
  })
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
    thinking: row('thinking')?.value,
    outputStyle: row('outputStyle')?.value,
  })
}

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await reportCatalog($)
    await reportStatus($)
    return started
  })

  on('command.run', async ($, e, next) => {
    const result = await next(e)
    const level = e.args.trim().toLowerCase()
    if (e.command === 'effort' && EFFORT_LEVELS.includes(level)) await post($, { effort: level })
    await reportCatalog($)
    await reportStatus($)
    return result
  })

  on('turn.start', async ($, e, next) => {
    waiting = false
    await post($, { status: 'busy' })
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    const done = await next(e)
    if (e.agentId !== undefined) return done
    waiting = false
    await post($, { status: 'idle' })
    await reportStatus($)
    return done
  })

  on('classic.PermissionRequest', async ($, e, next) => {
    waiting = true
    await post($, { status: 'waiting' })
    return next(e)
  })

  on('tool.call', { tool: 'AskUserQuestion' }, async ($, e, next) => {
    waiting = true
    await post($, { status: 'waiting' })
    return next(e)
  })

  on('tool.call', async ($, e, next) => {
    const result = await next(e)
    if (waiting) {
      waiting = false
      await post($, { status: 'busy' })
    }
    return result
  })

  on('classic.UserPromptSubmit', async ($, e, next) => {
    await post($, { permissionMode: e.permission_mode })
    return next(e)
  })

  on('classic.Stop', async ($, e, next) => {
    await post($, { permissionMode: e.permission_mode, effort: e.effort?.level })
    return next(e)
  })
}
