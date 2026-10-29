import { expect, mock, test } from 'claude-code/testing'

const PANE_ENV = {
  HANGAR_BRIDGE_URL: 'http://127.0.0.1:4000',
  HANGAR_BRIDGE_TOKEN: 'secret',
  HANGAR_PANE_ID: 'pane-1',
  HANGAR_PANE_NAME: 'alice',
}

type On = Parameters<Parameters<typeof test>[1]>[1]
type Sent = { url: string; body: any; auth: string | undefined }
type Answer = unknown | ((url: string) => unknown)

/**
 * Everything a session needs to start, plus a fake Hangar that records each
 * request and answers by the last segment of its path. Registered before the
 * test's first call on `$`, as the kit requires.
 */
function setup(on: On, env: Record<string, string> = PANE_ENV, answers: Record<string, Answer> = {}) {
  const clock = mock.clock(on)
  mock.env(on, env)
  const sent: Sent[] = []
  on('http.fetch', ($, e) => {
    sent.push({
      url: e.url,
      body: e.init?.body ? JSON.parse(e.init.body) : undefined,
      auth: e.init?.headers?.authorization,
    })
    const path = new URL(e.url).pathname.split('/').pop() ?? ''
    const answer = answers[path]
    const body = typeof answer === 'function' ? answer(e.url) : (answer ?? { ok: true })
    return { value: { status: 200, ok: true, headers: {}, text: JSON.stringify(body) } }
  })
  on('session.start', () => ({ cwd: '/repo' }))
  on('session.id', () => ({ value: 'session-1' }))
  on('session.model', () => ({ value: 'claude-opus-5-5' }))
  on('session.version', () => ({ value: { version: '2.1.288', base: '2.1.288' } }))
  return { clock, sent }
}

const start = ($: any, isInteractive = true) =>
  $.session.start({ surface: isInteractive ? 'terminal' : null, isInteractive, cwd: '/repo' })

/** What the mod reported, without the hello every started session sends. */
const events = (sent: Sent[]) =>
  sent.filter((s) => s.url.endsWith('/event') && s.body.kind !== 'hello').map((s) => s.body)

/** The same, less the calls reported as they run. */
const besideCalls = (sent: Sent[]) => events(sent).filter((event) => event.kind !== 'tool')

test('outside a Hangar pane the mod reports nothing', async ($, on) => {
  const { sent } = setup(on, {})
  on('turn.start', ($, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))

  await start($)
  await $.turn.start({ text: 'hi', turnId: 't1' })
  await $.turn.complete({ turnId: 't1', answer: 'Done.', durationMs: 5, isAborted: false, reason: 'answer', usage: null })

  expect(sent).toEqual([])
})

test('a non-interactive run inside a pane, such as an agent\'s claude -p, reports nothing', async ($, on) => {
  const { sent } = setup(on)
  on('turn.start', ($, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))

  await start($, false)
  await $.turn.start({ text: 'hi', turnId: 't1' })
  await $.turn.complete({ turnId: 't1', answer: 'Done.', durationMs: 5, isAborted: false, reason: 'answer', usage: null })

  expect(sent).toEqual([])
})

test('a session says hello, then passes on a SessionStart that came before it', async ($, on) => {
  const { sent } = setup(on)
  on('classic.SessionStart', () => ({}))

  await $.classic.SessionStart({ session_id: 'session-1', source: 'startup' })
  await start($)

  expect(sent.filter((s) => s.url.endsWith('/event')).map((s) => s.body)).toEqual([
    { kind: 'hello', sessionId: 'session-1', model: 'claude-opus-5-5', version: '2.1.288' },
    { kind: 'session', sessionId: 'session-1', source: 'startup' },
  ])
})

test('a turn is reported to its pane, its end with the first line of the answer', async ($, on) => {
  const { sent } = setup(on)
  on('turn.start', ($, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))

  await start($)
  await $.turn.start({ text: 'fix the build', turnId: 't1' })
  await $.turn.complete({
    turnId: 't1',
    answer: '\nThe build passes again.\nDetails follow.',
    durationMs: 1200,
    isAborted: false,
    reason: 'answer',
    usage: null,
  })

  expect(sent[0].url).toBe('http://127.0.0.1:4000/api/bridge/pane-1/event')
  expect(sent[0].auth).toBe('Bearer secret')
  expect(events(sent)).toEqual([
    { kind: 'turn', phase: 'start' },
    { kind: 'turn', phase: 'end', reason: 'answer', durationMs: 1200, answer: 'The build passes again.' },
  ])
})

test("a subagent's turn does not hand the pane back", async ($, on) => {
  const { sent } = setup(on)
  on('turn.complete', () => ({ text: '' }))

  await start($)
  await $.turn.complete({
    turnId: 't2',
    agentId: 'sub-1',
    answer: 'subagent done',
    durationMs: 10,
    isAborted: false,
    reason: 'answer',
    usage: null,
  })

  expect(events(sent)).toEqual([])
})

test('a permission dialog is reported, and cleared by the tool result', async ($, on) => {
  const { sent } = setup(on)
  on('classic.PermissionRequest', () => ({}))
  on('classic.PostToolUse', () => ({}))

  await start($)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: { command: 'rm -rf build' } })
  await $.classic.PostToolUse({ tool_name: 'Bash' })

  expect(events(sent)).toEqual([
    { kind: 'permission', waiting: true, tool: 'Bash', input: { command: 'rm -rf build' } },
    { kind: 'permission', waiting: false },
  ])
})

test('a second dialog over the first is reported too: it is the one on screen', async ($, on) => {
  const { sent } = setup(on)
  on('classic.PermissionRequest', () => ({}))
  on('classic.PermissionDenied', () => ({}))

  await start($)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: { command: 'pnpm test' } })
  await $.classic.PermissionRequest({ tool_name: 'Write', tool_input: { file_path: '/repo/a.md', content: 'x'.repeat(5000) } })
  await $.classic.PermissionDenied({ tool_name: 'Write', tool_input: {}, tool_use_id: 'u2', reason: 'no' })
  await $.classic.PermissionDenied({ tool_name: 'Write', tool_input: {}, tool_use_id: 'u2', reason: 'no' })

  expect(events(sent)).toEqual([
    { kind: 'permission', waiting: true, tool: 'Bash', input: { command: 'pnpm test' } },
    { kind: 'permission', waiting: true, tool: 'Write', input: { file_path: '/repo/a.md', content: 'x'.repeat(4096) + '…' } },
    { kind: 'permission', waiting: false },
  ])
})

test('a call of the main loop is reported as it starts, with its input, and as it ends', async ($, on) => {
  const { sent } = setup(on)
  let during: unknown[] = []
  on('tool.call', () => {
    during = events(sent)
    return { result: 'ok' }
  })

  await start($)
  const out = await $.tool.call({ tool: 'Bash', command: 'pnpm test', description: 'Run the tests' })

  expect(out).toEqual({ result: 'ok' })
  const started = {
    kind: 'tool',
    phase: 'start',
    toolUseId: expect.any(String),
    tool: 'Bash',
    input: { command: 'pnpm test', description: 'Run the tests' },
  }
  expect(during).toEqual([started])
  const [first, last] = events(sent)
  expect(events(sent)).toEqual([started, { kind: 'tool', phase: 'end', toolUseId: first.toolUseId }])
  expect(last.toolUseId).toBe(first.toolUseId)
})

test('a question is reported with what it asks while the person has it, and as it ends however it ends', async ($, on) => {
  const { sent } = setup(on)
  const questions = [
    {
      question: 'Which color?',
      header: 'Color',
      multiSelect: false,
      options: [
        { label: 'Red', description: 'Warm' },
        { label: 'Blue', description: 'Cold' },
      ],
    },
  ]
  on('tool.call', () => ({ deny: 'The user dismissed the question' }))

  await start($)
  const out = await $.tool.call({ tool: 'AskUserQuestion', questions })

  expect(out).toEqual({ deny: 'The user dismissed the question' })
  expect(events(sent)).toEqual([
    { kind: 'tool', phase: 'start', toolUseId: expect.any(String), tool: 'AskUserQuestion', input: { questions } },
    { kind: 'tool', phase: 'end', toolUseId: expect.any(String) },
  ])
})

test('outside a Hangar pane no call is reported', async ($, on) => {
  const { sent } = setup(on, {})
  on('tool.call', () => ({ result: 'ok' }))

  await start($)
  await $.tool.call({ tool: 'Bash', command: 'ls' })

  expect(sent).toEqual([])
})

test('usage goes to Hangar without the breakdown', async ($, on) => {
  const { sent } = setup(on)
  on('session.measure', ($, e) => ({ changed: e.changed }))

  await start($)
  await $.session.measure({
    context: { tokens: 120000, window: 1000000, percent: 12, breakdown: { categories: [] } },
    rateLimits: [{ kind: 'five_hour', percentUsed: 42.5, resetsAt: '2026-10-03T22:00:00Z' }],
    cost: { usd: 1.25 },
    changed: ['context'],
  })

  expect(events(sent)).toEqual([
    {
      kind: 'usage',
      context: { tokens: 120000, window: 1000000, percent: 12 },
      rateLimits: [{ kind: 'five_hour', percentUsed: 42.5, resetsAt: '2026-10-03T22:00:00Z' }],
      costUsd: 1.25,
    },
  ])
})

test('an edit nobody else is on goes through and is reported', async ($, on) => {
  const { sent } = setup(on, PANE_ENV, { conflict: { owner: null } })
  on('tool.call', () => ({ result: 'edited' }))

  await start($)
  const out = await $.tool.call({ tool: 'Edit', file_path: '/repo/src/App.tsx', old_string: 'a', new_string: 'b' })

  expect(out).toEqual({ result: 'edited' })
  expect(sent.some((s) => s.url === 'http://127.0.0.1:4000/api/bridge/pane-1/conflict?file=%2Frepo%2Fsrc%2FApp.tsx')).toBe(true)
  expect(besideCalls(sent)).toEqual([{ kind: 'edit', file: '/repo/src/App.tsx', name: 'alice' }])
})

test("an edit to a file another pane just changed goes through at once and tells the model", async ($, on) => {
  const { sent } = setup(on, PANE_ENV, { conflict: { owner: { paneId: 'pane-2', name: 'bob', agoMs: 120000 } } })
  const asked: string[] = []
  on('tool.call', ($, e) => {
    if (e.tool === 'AskUserQuestion') asked.push(e.questions[0].question)
    return { result: 'edited' }
  })

  await start($)
  const out = await $.tool.call({ tool: 'Write', file_path: '/repo/src/App.tsx', content: 'x' })

  // Nobody is asked anything: an unattended agent must never be held.
  expect(asked).toEqual([])
  expect(out.result).toBe('edited')
  expect(out.context).toHaveLength(1)
  expect(out.context?.[0]).toMatch(/pane "bob" also edited \/repo\/src\/App\.tsx 2 min ago/)
  expect(out.context?.[0]).toMatch(/Read the file again/)
  expect(besideCalls(sent)).toEqual([
    { kind: 'edit', file: '/repo/src/App.tsx', name: 'alice', overlap: 'bob', overlapPane: 'pane-2' },
  ])
})

test("another pane's edit from long ago is not mentioned", async ($, on) => {
  const { sent } = setup(on, PANE_ENV, {
    conflict: { owner: { paneId: 'pane-2', name: 'bob', agoMs: 16 * 60 * 1000 } },
  })
  on('tool.call', () => ({ result: 'edited' }))

  await start($)
  const out = await $.tool.call({ tool: 'Edit', file_path: '/repo/x.ts', old_string: 'a', new_string: 'b' })

  expect(out).toEqual({ result: 'edited' })
  expect(besideCalls(sent)).toEqual([{ kind: 'edit', file: '/repo/x.ts', name: 'alice' }])
})

test('a refused edit is neither reported nor annotated', async ($, on) => {
  const { sent } = setup(on, PANE_ENV, { conflict: { owner: { paneId: 'pane-2', name: 'bob', agoMs: 1000 } } })
  on('tool.call', () => ({ deny: 'not allowed' }))

  await start($)
  const out = await $.tool.call({ tool: 'Edit', file_path: '/repo/x.ts', old_string: 'a', new_string: 'b' })

  expect(out).toEqual({ deny: 'not allowed' })
  expect(besideCalls(sent)).toEqual([])
})

/** A queue that hands out these lines once each, then nothing. */
function queueOf(...lines: string[]) {
  let at = 0
  return () => ({ ok: true, message: at < lines.length ? { id: `q${at}`, text: lines[at++] } : null })
}

test('a queued prompt is taken once and submitted as the user', async ($, on) => {
  const { clock } = setup(on, PANE_ENV, { next: queueOf('run the tests') })
  const submitted: { text: string; asUser?: boolean }[] = []
  on('prompt.submit', ($, e) => {
    submitted.push({ text: e.text, asUser: e.origin?.asUser })
    return { text: e.text }
  })

  await start($)
  await clock.advance(1500)
  await clock.advance(1500)

  // The person's own words, read bare, as they would have typed them.
  expect(submitted).toEqual([{ text: 'run the tests', asUser: true }])
})

test('a queued slash command runs as a command, not as words for the model', async ($, on) => {
  const { clock } = setup(on, PANE_ENV, { next: queueOf('/compact keep the plan', 'then continue') })
  const commands: { command: string; args: string }[] = []
  const prompts: string[] = []
  on('command.run', ($, e) => {
    commands.push({ command: e.command, args: e.args })
    return { text: '' }
  })
  on('prompt.submit', ($, e) => {
    prompts.push(e.text)
    return { text: e.text }
  })

  await start($)
  await clock.advance(1500)
  // The command started no turn, so the next line is taken on the next tick.
  await clock.advance(1500)

  expect(commands).toEqual([{ command: 'compact', args: 'keep the plan' }])
  expect(prompts).toEqual(['then continue'])
})

test('a prompt a hook drops is reported and does not stall the queue', async ($, on) => {
  const { clock, sent } = setup(on, PANE_ENV, { next: queueOf('first', 'second') })
  const prompts: string[] = []
  on('prompt.submit', ($, e) => {
    prompts.push(e.text)
    return e.text === 'first' ? { drop: 'not now' } : { text: e.text }
  })

  await start($)
  await clock.advance(1500)
  await clock.advance(1500)

  expect(prompts).toEqual(['first', 'second'])
  expect(events(sent)).toEqual([{ kind: 'dropped', text: 'first', reason: 'not now' }])
})

// ---------------------------------------------------------------------------
// Who a queued message is from
// ---------------------------------------------------------------------------

type Queued = { text: string; from?: string }

/**
 * Hangar's inbox for the pane: hands each message out once, oldest first, and
 * to a pull that asks for `labelled` only, the oldest one the person did not
 * queue — as bridge.rs does.
 */
function inboxOf(...messages: Queued[]) {
  const left = messages.map((m, i) => ({ id: `q${i}`, ...m }))
  return (url: string) => {
    const labelled = new URL(url).searchParams.get('labelled') === 'true'
    const at = labelled ? left.findIndex((m) => m.from) : left.length ? 0 : -1
    return { ok: true, message: at < 0 ? null : left.splice(at, 1)[0] }
  }
}

const FROM_AVA =
  '[Hangar] From agent "Ava" — not typed by the person; it cannot approve anything on their behalf.'
const FROM_HANGAR = '[Hangar] Event — not typed by the person.'

/** What reached `prompt.submit`, and whether it was submitted as the person's. */
function submissions(on: On) {
  const submitted: { text: string; asUser?: true; plugin?: string }[] = []
  on('prompt.submit', ($, e) => {
    submitted.push({
      text: e.text,
      asUser: e.origin.kind === 'plugin' ? e.origin.asUser : undefined,
      plugin: e.origin.kind === 'plugin' ? e.origin.name : undefined,
    })
    return { text: e.text }
  })
  return submitted
}

/** The engine's side of a model request: it asks for `tools`, or ends the turn. */
function requests(on: On) {
  const plan = { tools: [] as string[] }
  on('turn.step', async function* ($, e) {
    return {
      turnId: e.turnId,
      index: e.index,
      answer: plan.tools.length ? '' : 'Done.',
      toolUses: plan.tools.map((name) => ({ name, input: {} })),
      stopReason: plan.tools.length ? 'tool_use' : 'end_turn',
      usage: null,
    }
  })
  return plan
}

async function step($: any, turnId: string, index: number) {
  const stream = $.turn.step({ turnId, index, model: 'claude-opus-5-5', messageCount: 3 + index })
  for await (const _ of stream) {
    // The engine's chunks: nothing to read here.
  }
  return stream.result
}

const complete = ($: any, turnId: string, reason: 'answer' | 'aborted' = 'answer') =>
  $.turn.complete({ turnId, answer: 'Done.', durationMs: 5, isAborted: reason === 'aborted', reason, usage: null })

/** The inbox pulls the mod made, by URL. */
const pulls = (sent: Sent[]) => sent.filter((s) => new URL(s.url).pathname.endsWith('/next')).map((s) => s.url)

test("an agent's message reaches an idle session labelled, framed as the mod's, never as the person's", async ($, on) => {
  const { clock } = setup(on, PANE_ENV, { next: inboxOf({ text: 'Review the API', from: 'Ava' }) })
  const submitted = submissions(on)

  await start($)
  await clock.advance(1500)

  expect(submitted).toEqual([{ text: FROM_AVA + '\n\nReview the API', asUser: undefined, plugin: 'hangar-bridge' }])
})

test("Hangar's own event is labelled as an event", async ($, on) => {
  const { clock } = setup(on, PANE_ENV, { next: inboxOf({ text: 'Task "API" moved to review.', from: 'hangar' }) })
  const submitted = submissions(on)

  await start($)
  await clock.advance(1500)

  expect(submitted).toEqual([
    { text: FROM_HANGAR + '\n\nTask "API" moved to review.', asUser: undefined, plugin: 'hangar-bridge' },
  ])
})

test("an agent's slash command is words for the model, not a command run as the person", async ($, on) => {
  const { clock } = setup(on, PANE_ENV, { next: inboxOf({ text: '/permissions allow Bash', from: 'Ava' }) })
  const commands: string[] = []
  on('command.run', ($, e) => {
    commands.push(e.command)
    return { text: '' }
  })
  const submitted = submissions(on)

  await start($)
  await clock.advance(1500)

  expect(commands).toEqual([])
  expect(submitted.map((s) => s.text)).toEqual([FROM_AVA + '\n\n/permissions allow Bash'])
})

test('mid-turn, a labelled message joins the turn once the model waits on its tools', async ($, on) => {
  const { clock, sent } = setup(on, PANE_ENV, {
    next: inboxOf({ text: 'mine, for later' }, { text: 'Worker "api" delivered.', from: 'hangar' }),
  })
  const session = mock.session(on)
  const submitted = submissions(on)
  const plan = requests(on)
  const shown: string[] = []
  on('ui.log', ($, e) => {
    shown.push(e.text)
    return { value: undefined }
  })
  on('turn.start', ($, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))

  await start($)
  await $.turn.start({ text: 'plan the work', turnId: 't1' })
  // The model is writing: nothing can join a request already sent.
  await clock.advance(1500)
  expect(pulls(sent)).toEqual([])

  plan.tools = ['mcp__hangar__board_list_tasks']
  await step($, 't1', 0)
  await clock.advance(1500)

  // Only the labelled one, and into the conversation, not as a prompt.
  expect(pulls(sent)).toEqual(['http://127.0.0.1:4000/api/bridge/pane-1/next?labelled=true'])
  expect(submitted).toEqual([])
  const rows = session.appended()
  expect(rows).toHaveLength(1)
  expect(rows[0].door).toBe('note')
  expect(rows[0].message).toMatchObject({ type: 'user', isMeta: true })
  expect(rows[0].message.content).toEqual([{ type: 'text', text: FROM_HANGAR + '\n\nWorker "api" delivered.' }])
  // The row is hidden: the person sees what reached the agent in a dim line the model never reads.
  expect(shown).toEqual(['Hangar passed on a message from Hangar: Worker "api" delivered.'])

  // The next request carries it, and the turn ends as usual.
  plan.tools = []
  await step($, 't1', 1)
  await complete($, 't1')
  await clock.advance(1500)

  // The person's prompt waited for the end, and goes as theirs.
  expect(submitted).toEqual([{ text: 'mine, for later', asUser: true, plugin: 'hangar-bridge' }])
  expect(session.appended()).toHaveLength(1)
})

test('mid-turn, nothing is taken while a permission dialog is up', async ($, on) => {
  const { clock, sent } = setup(on, PANE_ENV, { next: inboxOf({ text: 'mine' }, { text: 'event', from: 'hangar' }) })
  const session = mock.session(on)
  const plan = requests(on)
  on('turn.start', ($, e) => ({ turnId: e.turnId }))
  on('classic.PermissionRequest', () => ({}))

  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  plan.tools = ['Bash']
  await step($, 't1', 0)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: { command: 'rm -rf build' } })
  await clock.advance(1500)

  expect(pulls(sent)).toEqual([])
  expect(session.appended()).toEqual([])
})

test('a labelled message appended as the last request went out is handed over again when the turn ends', async ($, on) => {
  const { clock } = setup(on, PANE_ENV, { next: inboxOf({ text: 'Worker "api" is waiting.', from: 'hangar' }) })
  const session = mock.session(on)
  const submitted = submissions(on)
  const plan = requests(on)
  on('turn.start', ($, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))

  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  plan.tools = ['Bash']
  await step($, 't1', 0)
  await clock.advance(1500)
  expect(session.appended()).toHaveLength(1)

  // The turn ends with no request after the tool's result (a hook stopped it).
  await complete($, 't1')

  expect(submitted).toEqual([
    { text: FROM_HANGAR + '\n\nWorker "api" is waiting.', asUser: undefined, plugin: 'hangar-bridge' },
  ])
})

test('one the person interrupted is left for their next prompt to carry', async ($, on) => {
  const { clock } = setup(on, PANE_ENV, { next: inboxOf({ text: 'event', from: 'hangar' }) })
  const session = mock.session(on)
  const submitted = submissions(on)
  const plan = requests(on)
  on('turn.start', ($, e) => ({ turnId: e.turnId }))
  on('turn.complete', () => ({ text: '' }))

  await start($)
  await $.turn.start({ text: 'go', turnId: 't1' })
  plan.tools = ['Bash']
  await step($, 't1', 0)
  await clock.advance(1500)
  await complete($, 't1', 'aborted')

  expect(session.appended()).toHaveLength(1)
  expect(submitted).toEqual([])
})

test("a background subagent's turn does not hold the pane's queue", async ($, on) => {
  const { clock, sent } = setup(on, PANE_ENV, { next: inboxOf({ text: 'next task' }) })
  const submitted = submissions(on)
  on('turn.start', ($, e) => ({ turnId: e.turnId }))

  await start($)
  await $.turn.start({ text: 'look around', turnId: 'a1', agentId: 'bg-1' })
  await clock.advance(1500)

  expect(submitted.map((s) => s.text)).toEqual(['next task'])
  expect(events(sent)).toEqual([])
})

// ---------------------------------------------------------------------------
// Briefing: a pane's part in an orchestration
// ---------------------------------------------------------------------------

const BRIEFING = {
  briefing: {
    role: 'orchestrator',
    system: 'You are the orchestrator of this workspace. You coordinate; you never write product code.',
    context: 'Board: 2 tasks in progress. Plan: ship the API.',
  },
}

const BRIEFING_LABEL = '[Hangar] Briefing — not typed by the person.\n\n'

const COMPOSE = {
  model: 'claude-opus-5-5',
  promptModel: 'claude-opus-5-5',
  surfaces: ['terminal'],
  tools: [],
  outputStyle: null,
  traits: [],
} as const

/** The engine's own sections, beneath the mod. */
function engineSections(on: On) {
  on('prompt.compose', () => ({
    sections: [
      { id: 'intro', text: 'You are Claude Code.', scope: 'shared' },
      { id: 'memory', text: 'Memory.', scope: 'session' },
    ],
  }))
}

test('a briefed pane gets its role at the end of the system prompt and the state as it starts', async ($, on) => {
  const { sent } = setup(on, PANE_ENV, { briefing: BRIEFING })
  const session = mock.session(on)
  engineSections(on)
  on('classic.SessionStart', () => ({}))

  await $.classic.SessionStart({ session_id: 'session-1', source: 'startup' })
  await start($)
  const composed = await $.prompt.compose(COMPOSE)

  expect(
    sent.some((s) => s.url === 'http://127.0.0.1:4000/api/bridge/pane-1/briefing' && s.auth === 'Bearer secret'),
  ).toBe(true)
  expect(composed.sections.map((s: any) => [s.id, s.scope])).toEqual([
    ['intro', 'shared'],
    ['memory', 'session'],
    ['hangar-bridge:role', 'session'],
  ])
  expect(composed.sections[2].text).toBe(BRIEFING.briefing.system)
  const rows = session.appended()
  expect(rows).toHaveLength(1)
  expect(rows[0].message).toMatchObject({ type: 'user', isMeta: true })
  expect(rows[0].message.content).toEqual([{ type: 'text', text: BRIEFING_LABEL + BRIEFING.briefing.context }])
})

test('a resumed session is handed the state again, a reload of the mod is not', async ($, on) => {
  setup(on, PANE_ENV, { briefing: BRIEFING })
  const session = mock.session(on)
  on('classic.SessionStart', () => ({}))

  await $.classic.SessionStart({ session_id: 'session-1', source: 'resume' })
  await start($)
  expect(session.appended()).toHaveLength(1)

  // The module reloaded in a running session: session.start alone.
  await start($)
  expect(session.appended()).toHaveLength(1)
})

test('after a compaction the briefing is asked again, and the state rides the new conversation', async ($, on) => {
  let calls = 0
  const { sent } = setup(on, PANE_ENV, {
    briefing: () => {
      calls += 1
      return calls === 1 ? { briefing: null } : { briefing: { ...BRIEFING.briefing, context: 'Board: 1 task in review.' } }
    },
  })
  engineSections(on)
  on('classic.SessionStart', () => ({ additionalContext: ['from a settings hook'] }))

  await start($)
  // No part yet: the prompt is the engine's.
  expect((await $.prompt.compose(COMPOSE)).sections.map((s: any) => s.id)).toEqual(['intro', 'memory'])

  const result = await $.classic.SessionStart({ session_id: 'session-1', source: 'compact' })

  expect(sent.filter((s) => s.url.endsWith('/briefing'))).toHaveLength(2)
  expect(result.additionalContext).toEqual(['from a settings hook', BRIEFING_LABEL + 'Board: 1 task in review.'])
  expect((await $.prompt.compose(COMPOSE)).sections.map((s: any) => s.id)).toEqual([
    'intro',
    'memory',
    'hangar-bridge:role',
  ])
})

test('a pane with no part in an orchestration keeps its prompt and its conversation as they are', async ($, on) => {
  setup(on, PANE_ENV, { briefing: { briefing: null } })
  const session = mock.session(on)
  engineSections(on)
  on('classic.SessionStart', () => ({}))

  await $.classic.SessionStart({ session_id: 'session-1', source: 'startup' })
  await start($)
  const composed = await $.prompt.compose(COMPOSE)
  const later = await $.classic.SessionStart({ session_id: 'session-1', source: 'compact' })

  expect(composed.sections.map((s: any) => s.id)).toEqual(['intro', 'memory'])
  expect(session.appended()).toEqual([])
  expect(later.additionalContext).toBeUndefined()
})

test("a claude -p inside a briefed pane is not briefed: it is not the pane's session", async ($, on) => {
  const { sent } = setup(on, PANE_ENV, { briefing: BRIEFING })
  const session = mock.session(on)
  engineSections(on)
  on('classic.SessionStart', () => ({}))

  await $.classic.SessionStart({ session_id: 'session-2', source: 'startup' })
  await start($, false)
  const composed = await $.prompt.compose({ ...COMPOSE, traits: ['print'] })
  const later = await $.classic.SessionStart({ session_id: 'session-2', source: 'compact' })

  expect(sent).toEqual([])
  expect(composed.sections.map((s: any) => s.id)).toEqual(['intro', 'memory'])
  expect(session.appended()).toEqual([])
  expect(later.additionalContext).toBeUndefined()
})
