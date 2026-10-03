// hangar-bridge — tells Hangar.AI what the Claude Code session in one of its
// panes is doing, and takes prompts Hangar queued for it.
//
// Hangar loads this module through CLAUDE_CODE_PLUGIN_DIRS for the panes it
// spawns, and gives each one its pane id and a loopback URL + token in the
// environment. Anywhere else those variables are missing, and every hook below
// just passes its event through.
//
// Everything is reported, nothing is decided: no hook changes a tool call, a
// permission or a prompt, except the one that asks the user before editing a
// file another pane is editing — and only when the user answers to leave it.

/**
 * `{ base, token, pane, name }` once session.start has found this session is
 * a Hangar pane's own; null when it is not; undefined until then.
 */
let bridge
/** A SessionStart that came before session.start, held until it is decided. */
let pendingSession = null
/** A turn of the main loop is running. Queued prompts wait for it to end. */
let busy = false
/** A permission dialog or a question of ours is probably on screen. */
let waiting = false
/** One inbox request at a time: the timer does not wait for the last one. */
let polling = false

/** How recent another pane's edit must be to count as "editing it". */
const CONFLICT_WINDOW_MS = 15 * 60 * 1000

/**
 * Only the interactive session a pane started speaks for it. The variables are
 * inherited by everything that session starts — a `claude -p` an agent runs,
 * for one — and such a run reporting its turns would pass for the pane's. The
 * variables are left in place rather than cleared: a reload of this module
 * runs session.start again and must find them.
 */
async function connect($, isInteractive) {
  bridge = null
  if (!isInteractive) return
  const base = await $.env.get('HANGAR_BRIDGE_URL')
  const token = await $.env.get('HANGAR_BRIDGE_TOKEN')
  const pane = await $.env.get('HANGAR_PANE_ID')
  const name = await $.env.get('HANGAR_PANE_NAME')
  if (base && token && pane) bridge = { base, token, pane, name: name || pane }
}

function route(path) {
  return bridge.base + '/api/bridge/' + encodeURIComponent(bridge.pane) + path
}

function headers() {
  return { authorization: 'Bearer ' + bridge.token, 'content-type': 'application/json' }
}

async function send($, event) {
  if (!bridge) return
  try {
    await $.http.fetch(route('/event'), {
      method: 'POST',
      headers: headers(),
      body: JSON.stringify(event),
    })
  } catch {
    // Hangar is closing or restarting: the pane falls back to guessing from
    // its output, and the next event tries again.
  }
}

async function setWaiting($, value, detail) {
  if (waiting === value) return
  waiting = value
  await send($, { kind: 'permission', waiting: value, ...detail })
}

/** The first line of an answer is what a notification has room for. */
function headline(text) {
  const line = (text || '').split('\n').find((l) => l.trim()) || ''
  return line.length > 200 ? line.slice(0, 199) + '…' : line
}

async function poll($) {
  if (!bridge || busy || waiting || polling) return
  polling = true
  try {
    const res = await $.http.fetch(route('/next'), { headers: headers() })
    if (!res.ok) return
    const message = JSON.parse(res.text).message
    if (message && message.text) deliver($, message.text)
  } catch {
    // Unreachable for now; the next tick asks again.
  } finally {
    polling = false
  }
}

/**
 * Hands a queued line to the session the way it would have been typed: a
 * slash command runs as a command — submitted as a prompt it would reach the
 * model as the words "/compact" — and anything else is a prompt.
 *
 * `busy` holds the inbox until the line has run, so a second one is not taken
 * before the first has started; a prompt clears it through turn.complete, a
 * command when it resolves. Neither call is awaited: each waits for the
 * session to be idle, and this runs while it may not be.
 */
function deliver($, text) {
  busy = true
  const slash = /^\/(\S+)\s*([\s\S]*)$/.exec(text.trim())
  const run = slash
    ? $.command.run({ command: slash[1], args: slash[2] })
    : $.prompt.submit({ text, asUser: true })
  void run.then(
    (result) => {
      if (slash) busy = false
      // A hook refused the prompt: no turn starts, so nothing else would.
      if (result && result.drop) {
        busy = false
        void send($, { kind: 'dropped', text, reason: String(result.drop) })
      }
    },
    (err) => {
      busy = false
      void send($, { kind: 'dropped', text, reason: String(err) })
    },
  )
}

export function register(on) {
  on('session.start', async ($, e, next) => {
    await connect($, e.isInteractive)
    if (bridge) {
      const version = await $.session.version()
      await send($, {
        kind: 'hello',
        sessionId: await $.session.id(),
        model: await $.session.model(),
        version: version.version,
      })
      if (pendingSession) await send($, pendingSession)
      $.clock.every(1500, () => poll($))
    }
    pendingSession = null
    return next(e)
  })

  // Fires on the first start — before session.start, so it may have to wait
  // for it — and again after /clear, /resume, /branch and a compaction, which
  // is where the transcript a pane must resume can change.
  on('classic.SessionStart', async ($, e, next) => {
    const event = { kind: 'session', sessionId: e.session_id, source: e.source }
    if (bridge === undefined) pendingSession = event
    else await send($, event)
    return next(e)
  })

  on('turn.start', async ($, e, next) => {
    busy = true
    waiting = false
    await send($, { kind: 'turn', phase: 'start' })
    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    // A subagent's runs end as turns of their own; only the main loop hands
    // the pane back.
    if (e.agentId === undefined) {
      busy = false
      waiting = false
      await send($, {
        kind: 'turn',
        phase: 'end',
        reason: e.reason,
        durationMs: e.durationMs,
        answer: headline(e.answer),
      })
    }
    return next(e)
  })

  // The dialog is about to be shown. Nothing reports its answer, so the state
  // clears on what follows it: the tool's result, a denial, the turn's end —
  // and on the Hangar side, the user typing into the pane.
  on('classic.PermissionRequest', async ($, e, next) => {
    await setWaiting($, true, { tool: e.tool_name })
    return next(e)
  })

  on('classic.PostToolUse', async ($, e, next) => {
    await setWaiting($, false)
    return next(e)
  })

  on('classic.PostToolUseFailure', async ($, e, next) => {
    await setWaiting($, false)
    return next(e)
  })

  on('classic.PermissionDenied', async ($, e, next) => {
    await setWaiting($, false)
    return next(e)
  })

  on('session.measure', async ($, e, next) => {
    await send($, {
      kind: 'usage',
      context: {
        tokens: e.context.tokens ?? null,
        window: e.context.window,
        percent: e.context.percent ?? null,
      },
      rateLimits: e.rateLimits.map((limit) => ({
        kind: limit.kind,
        percentUsed: limit.percentUsed,
        resetsAt: limit.resetsAt ?? null,
      })),
      costUsd: e.cost ? e.cost.usd : null,
    })
    return next(e)
  })

  on('session.end', async ($, e, next) => {
    await send($, { kind: 'end', reason: e.reason })
    return next(e)
  })

  // Two agents editing one file overwrite each other. Before an edit, ask
  // Hangar whether another pane changed this file recently; if so, put it to
  // the user instead of deciding. No answer, or a dismissed question, lets
  // the edit through: the guard must never leave an agent stuck on its own.
  on('tool.call', { tool: ['Edit', 'Write', 'MultiEdit', 'NotebookEdit'] }, async ($, e, next) => {
    const file = e.file_path || e.notebook_path
    if (!file || !bridge) return next(e)

    let owner = null
    try {
      const res = await $.http.fetch(route('/conflict?file=' + encodeURIComponent(file)), {
        headers: headers(),
      })
      if (res.ok) owner = JSON.parse(res.text).owner
    } catch {
      owner = null
    }

    if (owner && owner.agoMs < CONFLICT_WINDOW_MS) {
      const minutes = Math.max(1, Math.round(owner.agoMs / 60000))
      const leave = 'Leave it to ' + owner.name
      let answer = 'Edit it anyway'
      await setWaiting($, true, { tool: e.tool, conflict: owner.name, conflictPane: owner.paneId })
      try {
        answer = await $.ui.ask(
          'The agent in pane "' + owner.name + '" edited ' + file + ' ' + minutes +
            ' min ago. Edit it anyway?',
          ['Edit it anyway', leave],
        )
      } catch {
        answer = 'Edit it anyway'
      }
      await setWaiting($, false)
      if (answer === leave) {
        return {
          deny:
            'The user asked to leave ' + file + ' to the agent in pane "' + owner.name +
            '", which is editing it. Coordinate with it first (message @' + owner.name +
            ') or work on something else.',
        }
      }
    }

    const result = await next(e)
    if (!result.deny && !result.isError) {
      await send($, { kind: 'edit', file, name: bridge.name })
    }
    return result
  })
}
