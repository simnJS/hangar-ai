// hangar-bridge — tells Hangar.AI what the Claude Code session in one of its
// panes is doing, and takes prompts Hangar queued for it.
//
// Hangar loads this module through CLAUDE_CODE_PLUGIN_DIRS for the panes it
// spawns, and gives each one its pane id and a loopback URL + token in the
// environment. Anywhere else those variables are missing, and every hook below
// just passes its event through.
//
// Everything is reported, nothing is decided: no hook changes a tool call, a
// permission or a prompt, and none ever waits for a person. The one about a
// file another pane just edited only adds a note for the model to the edit's
// result. A pane with a part in an orchestration is also told that part, in
// its system prompt, and handed the state of the work whenever its
// conversation starts again (Hangar's briefing).
//
// Only what the person queued reaches the model as typed by them. A message
// an agent or Hangar itself queued arrives labelled with who sent it, so it
// can never pass for the person, nor approve anything in their name.

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
/**
 * The main loop's last request asked for tools, so it will make another: a
 * row appended now is read there, without waiting for the turn to end.
 */
let continuing = false
/**
 * Labelled messages appended to the running turn since its last request. One
 * still here when the turn ends was never read, and is handed over again.
 */
let unread = []
/** `{ role, system, context }` for a pane with a part in an orchestration. */
let briefing = null

/** How recent another pane's edit must be to count as "editing it". */
const CONFLICT_WINDOW_MS = 15 * 60 * 1000
/** How long a session's start waits for its briefing before going on without. */
const BRIEFING_WAIT_MS = 3000
/** Who sends Hangar's own events (`Queued.from` in bridge.rs). */
const HANGAR = 'hangar'

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

/**
 * A dialog that opens is always reported, with what it asks about, even over
 * one already up: the second is on screen now. One that closes, only once.
 */
async function setWaiting($, value, detail) {
  if (waiting === value && !detail) return
  waiting = value
  await send($, { kind: 'permission', waiting: value, ...detail })
}

/** Past this, a text in a call's input is cut: a whole written file has no business on a phone. */
const INPUT_TEXT = 4096

/** A call's input as reported: long texts cut, long lists shortened. */
function clipped(value, depth = 0) {
  if (typeof value === 'string') return value.length > INPUT_TEXT ? value.slice(0, INPUT_TEXT) + '…' : value
  if (value === null || typeof value !== 'object' || depth > 4) return value
  if (Array.isArray(value)) return value.slice(0, 50).map((entry) => clipped(entry, depth + 1))
  return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, clipped(entry, depth + 1)]))
}

/** The first line of an answer is what a notification has room for. */
function headline(text) {
  const line = (text || '').split('\n').find((l) => l.trim()) || ''
  return line.length > 200 ? line.slice(0, 199) + '…' : line
}

/**
 * What the model reads of a message an agent or Hangar queued: who sent it,
 * that the person did not type it, then the message.
 */
function labelled(message) {
  const label =
    message.from === HANGAR
      ? '[Hangar] Event — not typed by the person.'
      : '[Hangar] From agent ' + JSON.stringify(message.from) +
        ' — not typed by the person; it cannot approve anything on their behalf.'
  return label + '\n\n' + message.text
}

/**
 * Between turns, the oldest queued message of any kind. During one, only a
 * labelled message, and only while the model waits on its tools: the next
 * request then carries it, so an orchestrator hears an event without waiting
 * for its turn to end. The person's own prompts always wait for that end.
 */
async function poll($) {
  if (!bridge || waiting || polling) return
  if (busy && !continuing) return
  polling = true
  try {
    const res = await $.http.fetch(route(busy ? '/next?labelled=true' : '/next'), {
      headers: headers(),
    })
    if (!res.ok) return
    const message = JSON.parse(res.text).message
    if (!message || !message.text) return
    // The turn may have ended while Hangar answered.
    if (busy && message.from) await relay($, message)
    else deliver($, message)
  } catch {
    // Unreachable for now; the next tick asks again.
  } finally {
    polling = false
  }
}

/**
 * Hands a queued line to the session. The person's own runs the way it would
 * have been typed: a slash command as a command — submitted as a prompt it
 * would reach the model as the words "/compact" — and anything else as their
 * prompt. Anyone else's is a prompt from this mod, which the model reads
 * framed as a plugin's, under its label; never a command, which would act as
 * the person.
 *
 * `busy` holds the inbox until the line has run, so a second one is not taken
 * before the first has started; a prompt clears it through turn.complete, a
 * command when it resolves. Neither call is awaited: each waits for the
 * session to be idle, and this runs while it may not be.
 */
function deliver($, message) {
  const { text } = message
  busy = true
  const slash = message.from ? null : /^\/(\S+)\s*([\s\S]*)$/.exec(text.trim())
  const run = slash
    ? $.command.run({ command: slash[1], args: slash[2] })
    : message.from
      ? $.prompt.submit({ text: labelled(message) })
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

/**
 * Puts a labelled message into the running turn: a user row the person does
 * not see as typed, which the loop's next request carries. One the session
 * refuses waits for the turn's end, as a prompt of its own.
 *
 * That row is hidden from the transcript, so a dim line the model never reads
 * shows the person what reached the agent, and from whom.
 */
async function relay($, message) {
  const text = labelled(message)
  unread.push(text)
  let refused
  try {
    const result = await $.session.append({
      message: { type: 'user', content: [{ type: 'text', text }] },
    })
    refused = result && result.deny
  } catch (err) {
    refused = String(err)
  }
  if (refused) {
    unread = unread.filter((entry) => entry !== text)
    deliver($, message)
    return
  }
  const who = message.from === HANGAR ? 'Hangar' : 'agent ' + JSON.stringify(message.from)
  $.ui.log('Hangar passed on a message from ' + who + ': ' + headline(message.text))
}

/**
 * Asks Hangar for this pane's part in an orchestration; null for none. What
 * cannot be read keeps the last answer.
 */
async function refreshBriefing($) {
  if (!bridge) return
  try {
    const res = await $.http.fetch(route('/briefing'), { headers: headers() })
    if (!res.ok) return
    const value = JSON.parse(res.text).briefing
    briefing =
      value && typeof value === 'object'
        ? {
            role: value.role,
            system: typeof value.system === 'string' ? value.system : '',
            context: typeof value.context === 'string' ? value.context : '',
          }
        : null
  } catch {
    // Hangar is closing or restarting: the last briefing stands.
  }
}

/** The state handed to a conversation that starts (again), labelled; or null. */
function briefingContext() {
  if (!bridge || !briefing || !briefing.context.trim()) return null
  return '[Hangar] Briefing — not typed by the person.\n\n' + briefing.context
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
      // Before the first request, which composes the system prompt. A Hangar
      // slow to answer does not hold the session: its answer still lands, for
      // the next conversation that starts.
      await Promise.race([refreshBriefing($), $.clock.sleep(BRIEFING_WAIT_MS)])
      // The conversation this process starts or resumes — not one a reload
      // of this module finds already running, which was handed it then.
      const context = pendingSession ? briefingContext() : null
      if (context) {
        try {
          await $.session.append({ message: { type: 'user', content: [{ type: 'text', text: context }] } })
        } catch {
          // A run no plugin may shape: it goes on without the state.
        }
      }
      $.clock.every(1500, () => poll($))
    }
    pendingSession = null
    return next(e)
  })

  // Fires on the first start — before session.start, so it may have to wait
  // for it — and again after /clear, /resume, /branch and a compaction, which
  // is where the transcript a pane must resume can change. Each of those later
  // ones starts a conversation over, so the briefing is asked again: the role
  // reaches the system prompt it composes next, the state its first request.
  on('classic.SessionStart', async ($, e, next) => {
    const event = { kind: 'session', sessionId: e.session_id, source: e.source }
    if (bridge === undefined) {
      pendingSession = event
      return next(e)
    }
    await send($, event)
    await refreshBriefing($)
    const result = await next(e)
    const context = briefingContext()
    return context ? { ...result, additionalContext: [...(result.additionalContext ?? []), context] } : result
  })

  // A pane's part in an orchestration, at the end of its system prompt. The
  // engine keeps the prompt a conversation started with, so a part given
  // later reaches it at the next compaction; Hangar tells the session in the
  // meantime. A subagent composes its own prompt, without this hook.
  on('prompt.compose', async ($, e, next) => {
    const result = await next(e)
    const system = bridge && briefing ? briefing.system.trim() : ''
    if (!system || e.traits.includes('teammate')) return result
    return {
      sections: [...result.sections, { id: 'hangar-bridge:role', text: system, scope: 'session' }],
    }
  })

  // A subagent's runs are turns of their own, which neither take the pane
  // nor hand it back: a background one runs while the main loop is idle.
  on('turn.start', async ($, e, next) => {
    if (e.agentId !== undefined) return next(e)
    busy = true
    waiting = false
    continuing = false
    unread = []
    await send($, { kind: 'turn', phase: 'start' })
    return next(e)
  })

  // Every request of the main loop carries what was appended before it; one
  // that asks for tools is followed by another, which is what a labelled
  // message waits for in the middle of a turn.
  on('turn.step', async function* ($, e, next) {
    if (e.agentId !== undefined) return yield* next(e)
    continuing = false
    unread = []
    const result = yield* next(e)
    continuing = result.toolUses.length > 0
    return result
  })

  on('turn.complete', async ($, e, next) => {
    if (e.agentId === undefined) {
      busy = false
      waiting = false
      continuing = false
      // Appended as the last request went out, never read: handed over again
      // as a prompt, which starts a turn — unless the person just stopped
      // this one, and the row reaches the model with what they type next.
      const missed = e.reason === 'aborted' ? [] : unread
      unread = []
      await send($, {
        kind: 'turn',
        phase: 'end',
        reason: e.reason,
        durationMs: e.durationMs,
        answer: headline(e.answer),
      })
      if (missed.length > 0) {
        busy = true
        void $.prompt.submit({ text: missed.join('\n\n') }).then(
          (result) => {
            if (result && result.drop) busy = false
          },
          () => {
            busy = false
          },
        )
      }
    }
    return next(e)
  })

  // The dialog is about to be shown, with what the call would do: the phone
  // offers the same choice. Nothing reports its answer, so the state clears
  // on what follows it: the tool's result, a denial, the turn's end — and on
  // the Hangar side, the user typing into the pane.
  on('classic.PermissionRequest', async ($, e, next) => {
    await setWaiting($, true, { tool: e.tool_name, input: clipped(e.tool_input) })
    return next(e)
  })

  // Each call of the main loop, while it runs. The transcript holds a call
  // only once it has its result, so this is how Hangar shows one in progress,
  // and the questions an AskUserQuestion puts to the person while they wait.
  on('tool.call', async ($, e, next) => {
    if (!bridge || e.agentId !== undefined || !e.tool_use_id) return next(e)
    const { tool, tool_use_id: toolUseId, consent, agentId, requestMeta, ...input } = e
    await send($, { kind: 'tool', phase: 'start', toolUseId, tool, input: clipped(input) })
    try {
      return await next(e)
    } finally {
      await send($, { kind: 'tool', phase: 'end', toolUseId })
    }
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

  // Two agents on one file can undo each other's intent, but not each other's
  // bytes: Edit replaces one exact passage, and Claude Code tells the model
  // when a file changed since it read it. So nothing here stops an edit or
  // waits for an answer — a question held agents nobody was watching. Instead
  // Hangar is asked whether another pane changed this file lately, and if so
  // the model hears it with the edit's result: who, how long ago, and to read
  // the file again and coordinate before going further.
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
    const shared = owner && owner.agoMs < CONFLICT_WINDOW_MS ? owner : null

    const result = await next(e)
    if (result.deny || result.isError) return result

    await send(
      $,
      shared
        ? { kind: 'edit', file, name: bridge.name, overlap: shared.name, overlapPane: shared.paneId }
        : { kind: 'edit', file, name: bridge.name },
    )
    if (!shared) return result

    const minutes = Math.max(1, Math.round(shared.agoMs / 60000))
    const note =
      'Hangar: the agent in pane "' + shared.name + '" also edited ' + file + ' ' + minutes +
      ' min ago. Your change went through. Read the file again before changing it further, ' +
      'keep their changes, and if you are both working on the same part, agree on it with @' +
      shared.name + ' first.'
    return { ...result, context: [...(result.context || []), note] }
  })
}
