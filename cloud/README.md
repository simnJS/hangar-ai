# Hangar Cloud

The hosted backend for the Hangar.AI shared board.

The desktop app keeps its kanban in `.hangar/board.json` and serves it from a
local HTTP server; agents drive it through the `board_*` MCP tools, and a mutex
in `src-tauri/src/board.rs` is what stops two of them claiming the same task.
That works for one machine. Hangar Cloud is the same board for a team: several
people, several machines, the agents running on each of them, one board.

It is a standalone Next.js app inside the Hangar repository, deployed as its
**own Vercel project** — it shares nothing with the marketing site at the
repository root.

## Design notes

**Wire compatibility.** Task and comment payloads are field-for-field what
`board.rs` emits: snake_case keys, `depends_on`, comments embedded in their
task, `release` as the way to hand a task back. `version` and `assignee_kind`
are the only additions, and the desktop board ignores fields it does not know.
Unknown keys in a request body are dropped rather than rejected, which is what
serde does on the Rust side.

**No interactive transactions.** The neon-http driver speaks to Neon over
fetch: one round trip per statement, no session. Every invariant two callers
could break at once is therefore a single conditional statement:

| Invariant | Statement |
|---|---|
| One agent per task | `UPDATE tasks SET assignee=…, column=CASE WHEN column='todo' THEN 'doing' ELSE column END … WHERE id=… AND (assignee IS NULL OR assignee=:agent)` — no row back means someone else holds it, and the caller gets a 409 naming the owner. The holder re-claiming its own task succeeds, so a retry after a dropped connection is safe |
| No lost update | `UPDATE tasks … WHERE version = :expected_version` when the caller passes one |
| A new card lands at the bottom | `INSERT … VALUES (…, (SELECT greatest(coalesce(max("order"),0),0)+1 FROM tasks WHERE board_id=… AND column=…))` |
| One membership per (team, user) | composite primary key |
| One row per token hash | unique index |

Bumping `boards.rev` and appending to `activity` happen *after* a successful
mutation, in a `db.batch` (neon runs a batch in a transaction). They are
deliberately not part of the mutation: a revision is a polling hint and the
activity feed is a journal, so a crash in between costs a client one late
refresh, never a task claimed twice. See `src/lib/mutations.ts`.

**Polling on one number.** `GET /boards/:id` answers with
`ETag: "<rev>"`. A client sending `If-None-Match` gets a 304 after a single row
is read — the task table is never touched. Authenticating the request already
loads the board row, so a no-change poll is one query.

**The build needs no environment.** `getDb()` builds the drizzle client on
first use and caches it in a module-level `let` (never a Proxy — that breaks
libraries that introspect the client). The home page is static and pulls in
nothing from Clerk. `next build` therefore passes with no `DATABASE_URL` and no
Clerk keys, which is what keeps CI and preview builds honest.

## Authentication

Two families of callers, one API.

- **People** sign in through Clerk. `clerkMiddleware()` (v7 / Core 3) only
  attaches the auth state; it protects nothing, because a middleware that
  redirected anonymous traffic would break every agent. Each route calls
  `requireUser()` and answers 401 as JSON.
- **Machines** send `Authorization: Bearer hgr_…`. A board token is scoped to
  exactly one board, so a leaked token cannot reach the rest of an account.
  Format: `hgr_` + 32 random bytes, base64url. Only the SHA-256 of the full
  token is stored; the plaintext is returned once, at creation, and cannot be
  recovered. `last_used_at` is refreshed at most once a minute, after the
  response.

Board routes accept either (`requireBoardAccess`), which is how the dashboard
and the agents share the same endpoints. When a request carries an
`Authorization` header it is judged on that alone — a revoked token fails
instead of quietly falling back to a session the browser also happens to hold.

## Routes

All under `/api/v1`. Errors are `{ "error": string, "code"?: string }` with
400 (validation), 401, 403, 404, 409 (claim taken, version conflict, last
owner). Stack traces never reach the response.

### Clerk session

| Method | Path | Notes |
|---|---|---|
| GET | `/me` | Creates the user row on first sight, refreshes the profile |
| GET / POST | `/teams` | List mine (with my role) / create (caller becomes owner) |
| PATCH | `/teams/:teamId` | Rename — owner |
| GET / POST | `/teams/:teamId/members` | List / add by `{ email }` — owner. 404 if that person has no account yet: emailed invitations are out of scope |
| DELETE | `/teams/:teamId/members/:userId` | Owner; refuses to remove the last owner (409) |
| GET / POST | `/teams/:teamId/boards` | List / create — any member |
| DELETE | `/boards/:boardId` | Owner. Cascades to tasks, comments, tokens, activity |
| GET / POST | `/boards/:boardId/tokens` | Metadata / mint (`{ token: "hgr_…" }`, once) — any member |
| DELETE | `/boards/:boardId/tokens/:tokenId` | Revoke (stamps `revoked_at`, keeps the row) |

### Board token **or** team member

| Method | Path | Notes |
|---|---|---|
| GET | `/boards/:boardId` | `{ board: { id, name, rev }, tasks: [...] }`, `ETag`/`If-None-Match` → 304 |
| POST | `/boards/:boardId/tasks` | `{ title, description?, column?, priority?, labels?, depends_on?, id? }` → 201 |
| PATCH | `/boards/:boardId/tasks/:taskId` | `{ title?, description?, column?, priority?, assignee?, release?, labels?, depends_on?, order?, expected_version? }`. `release` wins over `assignee` |
| DELETE | `/boards/:boardId/tasks/:taskId` | |
| POST | `/boards/:boardId/tasks/:taskId/claim` | `{ agent, kind? }`. Idempotent for the holder; 409 `{ error, code: "already_claimed", owner }` for anyone else. Only a `todo` task moves to `doing` |
| GET / POST | `/boards/:boardId/tasks/:taskId/comments` | `{ author, author_kind?, text }` |
| GET | `/boards/:boardId/activity` | `?limit=50&before=<epoch ms>`, newest first |
| GET | `/boards/:boardId/next-task` | Highest-priority unassigned `todo` task whose dependencies are all `done`, or `{ "task": null }` |

## Environment variables

Three, all provisioned by the Vercel integrations below — none of them are
needed to build.

| Name | From | Used by |
|---|---|---|
| `DATABASE_URL` | Neon integration | `getDb()`, drizzle-kit |
| `CLERK_SECRET_KEY` | Clerk integration | `auth()`, `currentUser()` |
| `NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY` | Clerk integration | `clerkMiddleware()` |

## Provisioning

From a clean checkout, with the Vercel CLI:

```sh
npm i -g vercel
vercel login

cd cloud
vercel link            # create a NEW project — do NOT link to the site project
vercel integration add neon --yes
vercel integration add clerk
vercel env pull .env.local --yes

npx dotenv -e .env.local -- npx drizzle-kit push
vercel deploy --prod
```

Notes:

- `vercel link` must create a new project. The repository root already belongs
  to the Hangar site project; linking `cloud/` to it would replace the site
  with this app. In the Vercel dashboard the new project's **Root Directory**
  must be `cloud`.
- `drizzle-kit push` applies `src/db/schema.ts` straight to the database, which
  is what you want for the first deploy. The generated SQL lives in
  `drizzle/` and is committed, so the schema is reviewable in a diff.
- Node.js runtime, not edge — `@neondatabase/serverless` and `node:crypto` both
  want it, and it is the default here.

## Local development

```sh
npm install
vercel env pull .env.local --yes   # or write the three variables by hand
npm run dev
```

| Script | What it does |
|---|---|
| `npm run dev` / `build` / `start` | Next.js |
| `npm test` | vitest, pure logic, no database and no network |
| `npm run typecheck` | `tsc --noEmit` |
| `npm run db:generate` | New SQL migration in `drizzle/` from the schema |
| `npm run db:push` | Apply the schema to the database |
| `npm run db:studio` | Drizzle Studio |

The `db:*` scripts go through `dotenv-cli`: drizzle-kit does not read
`.env.local` on its own, so they need `.env.local` to exist.

## Smoke test

There is no dashboard yet, so the session-authenticated calls are easiest from
the browser console of the deployment you are signed into (the Clerk cookie
travels with `credentials: "include"`):

```js
const api = (path, init) =>
  fetch(`/api/v1${path}`, {
    credentials: "include",
    headers: { "content-type": "application/json" },
    ...init,
  }).then((r) => r.json());

const { team } = await api("/teams", { method: "POST", body: '{"name":"Hangar"}' });
const { board } = await api(`/teams/${team.id}/boards`, { method: "POST", body: '{"name":"Main"}' });
const minted = await api(`/boards/${board.id}/tokens`, { method: "POST", body: '{"name":"Simon\'s PC"}' });
console.log(board.id, minted.token); // the token is shown once
```

Then, from a terminal — this is the path every agent takes:

```sh
BOARD=<board id>
TOKEN=hgr_<the token>
BASE=https://<your-deployment>/api/v1

# empty board, note the ETag
curl -si "$BASE/boards/$BOARD" -H "Authorization: Bearer $TOKEN" | grep -i etag

# create a task
curl -s -X POST "$BASE/boards/$BOARD/tasks" \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' \
  -d '{"title":"Smoke test","priority":3}'

# what should I work on?
curl -s "$BASE/boards/$BOARD/next-task" -H "Authorization: Bearer $TOKEN"

# claim it, then re-claim as the same agent (200), then as another one (409)
TASK=<task id>
curl -s -X POST "$BASE/boards/$BOARD/tasks/$TASK/claim" \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"agent":"claude-1"}'
curl -s -o /dev/null -w 'retry by the holder: %{http_code}\n' -X POST "$BASE/boards/$BOARD/tasks/$TASK/claim" \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"agent":"claude-1"}'
curl -s -w '\n' -X POST "$BASE/boards/$BOARD/tasks/$TASK/claim" \
  -H "Authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"agent":"claude-2"}'
# -> {"error":"already claimed by claude-1","code":"already_claimed","owner":"claude-1"}

# a client that is up to date gets 304 (rev is the number from the ETag above)
curl -s -o /dev/null -w '%{http_code}\n' "$BASE/boards/$BOARD" \
  -H "Authorization: Bearer $TOKEN" -H 'If-None-Match: "<rev>"'
```

## Known limits

- Adding a member only works for someone who already signed in once; there is
  no email invitation flow.
- No dashboard yet — the session routes exist, the UI is a separate task.
- Nothing garbage-collects `activity`; a busy board grows it forever.
- No rate limiting. Board tokens are the only throttle, and they are revocable.
