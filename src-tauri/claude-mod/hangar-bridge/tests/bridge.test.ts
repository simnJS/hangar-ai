import { expect, mock, test } from 'claude-code/testing'

const PANE_ENV = {
  HANGAR_BRIDGE_URL: 'http://127.0.0.1:4000',
  HANGAR_BRIDGE_TOKEN: 'secret',
  HANGAR_PANE_ID: 'pane-1',
  HANGAR_PANE_NAME: 'alice',
}

type On = Parameters<Parameters<typeof test>[1]>[1]
type Sent = { url: string; body: any; auth: string | undefined }
type Answer = unknown | (() => unknown)

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
    const body = typeof answer === 'function' ? answer() : (answer ?? { ok: true })
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

  expect(sent.map((s) => s.body)).toEqual([
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
    { kind: 'permission', waiting: true, tool: 'Bash' },
    { kind: 'permission', waiting: false },
  ])
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
  expect(events(sent)).toEqual([{ kind: 'edit', file: '/repo/src/App.tsx', name: 'alice' }])
})

test("asked about another pane's file, leaving it refuses the edit", async ($, on) => {
  const { sent } = setup(on, PANE_ENV, { conflict: { owner: { paneId: 'pane-2', name: 'bob', agoMs: 120000 } } })
  on('tool.call', ($, e) =>
    e.tool === 'AskUserQuestion'
      ? { result: { answers: { [e.questions[0].question]: 'Leave it to bob' } } }
      : { result: 'edited' },
  )

  await start($)
  const out = await $.tool.call({ tool: 'Write', file_path: '/repo/src/App.tsx', content: 'x' })

  expect(out.deny).toMatch(/pane "bob"/)
  expect(events(sent)).toEqual([
    { kind: 'permission', waiting: true, tool: 'Write', conflict: 'bob', conflictPane: 'pane-2' },
    { kind: 'permission', waiting: false },
  ])
})

test("asked about another pane's file, editing anyway lets it through", async ($, on) => {
  const { sent } = setup(on, PANE_ENV, { conflict: { owner: { paneId: 'pane-2', name: 'bob', agoMs: 60000 } } })
  on('tool.call', ($, e) =>
    e.tool === 'AskUserQuestion'
      ? { result: { answers: { [e.questions[0].question]: 'Edit it anyway' } } }
      : { result: 'edited' },
  )

  await start($)
  const out = await $.tool.call({ tool: 'Edit', file_path: '/repo/x.ts', old_string: 'a', new_string: 'b' })

  expect(out).toEqual({ result: 'edited' })
  expect(events(sent).at(-1)).toEqual({ kind: 'edit', file: '/repo/x.ts', name: 'alice' })
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

  expect(submitted.map((s) => s.text)).toEqual(['run the tests'])
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
