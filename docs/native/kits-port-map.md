# Porting the kits to native tabs

The twelve public kits of `hangar-ai` (cloned in `~/dev/kits/`), each ported to a native tab on a
branch `rust-rewrite` of its own repository. This page is what a porter needs and nothing else: the
recipe, then each kit's tabs, what maps directly, the gaps and the API that fills each one. The
contract is [kits-native.md](kits-native.md); the HTML library is [kit-ui.md](../kit-ui.md).

A port never touches the hangar repository: everything below is in `hangar-kit-sdk` on
`feat/native-rust`. A kit's plugin parts (MCP servers, hooks, skills, `tools/*.js`) stay as they are.

## The recipe

1. **Layout.** In the kit's repository, a crate beside the page, which stays:

   ```text
   hangar-kit.json        the HTML entry kept, a `native` field added (4)
   tabs/ci/…              the page, unchanged: what Hangar 1.x opens
   tabs/ci.wasm           the module, built and committed (3)
   native/Cargo.toml      [lib] crate-type = ["cdylib", "rlib"]
   native/Cargo.lock      committed
   native/src/lib.rs      the Kit and export_kit!
   native/src/logic.rs    logic.js, ported: pure, no SDK types where it can
   native/tests/*.rs      the cases of test/*.test.js, ported (5)
   scripts/build-native.sh
   ```

   Two tabs of one kit (kit-github) share one crate and one module: both tabs' `native` name
   `tabs/github.wasm`, and `init` reads `start.tab.id`.

2. **The SDK, from git** until a release tag holds it, and a small release build:

   ```toml
   [dependencies]
   hangar-kit-sdk = { git = "https://github.com/hangar-ai/hangar", branch = "feat/native-rust" }
   serde = { version = "1", features = ["derive"] }
   serde_json = "1"

   [profile.release]
   opt-level = "s"
   lto = true
   strip = true
   ```

   The SDK pulls in hangar-kit-ui and serde only: no iced, nothing that needs the system.

3. **Build and commit the module.** Hangar runs the `.wasm` a kit ships and never builds one, so a
   kit installed by cloning works as it is. `scripts/build-native.sh`:

   ```sh
   #!/bin/sh
   set -eu
   cd "$(dirname "$0")/.."
   cargo build --manifest-path native/Cargo.toml --release --target wasm32-unknown-unknown
   cp native/target/wasm32-unknown-unknown/release/kit_ci_tab.wasm tabs/ci.wasm
   ```

   `rustup target add wasm32-unknown-unknown` once; never `wasm32-wasip1`, never wasm-bindgen.
   Add `native/target/` to `.gitignore`, commit `tabs/ci.wasm` with the source that made it, and
   rebuild it in the commit that changes the source. A module is about 300 to 600 KB.

4. **The manifest.** Stay at `"schema": 1`, keep `entry` (the HTML page) and add `native`; leave
   `hangar` as it is (`>=1.9.0`): Hangar 1.x ignores the field and opens the page, the native Hangar
   opens the module, and the page again only when the module cannot load (the bar says so).

   ```json
   { "id": "ci", "title": "CI", "entry": "tabs/ci/index.html", "native": "tabs/ci.wasm",
     "permissions": ["command:ci-run", "command:ci-list", "command:ci-log", "notify"] }
   ```

   Port the kit's `test/manifest.test.js` cases for the new field (the file exists, is a `.wasm`,
   inside the kit).

5. **Test with the harness**, on the host, as `cargo test --manifest-path native/Cargo.toml`. Every
   case of the tab's `logic`, `controller` and `view` tests becomes a Rust test with the same names
   and expectations: `Harness::<Ci>::new(testing::start().grant([...]).param("id", "42"))`, answer
   calls with `reply_to("command.run", Ok(json!({...})))` (the recorded fixtures of `test/` are the
   answers), act with `press`, `type_into`, `choose`, `toggle`, `shortcut`, `draw`, `range`,
   `timer`, `elapse`, and read with `shows`, `texts`, `methods`, `calls`, `take_operations`.
   `testing::set_now(Some(ms))` fixes the clock; `.utc_offset(60)` and `.locale("fr")` the person.
   Keep the Node tests of the page, the hooks and the tools: they still run in `run-tests.js`.

6. **Try it in a dev Hangar.** In the hangar repository, `scripts/native-dev.sh` (its own data under
   `target/dev-home/`), then Settings → Kits → link the kit's folder (read live), open the tab; after
   each `build-native.sh`, reload the tab from its bar (a module whose file changed is compiled
   again). Without Hangar: `cargo run -p hangar-kit-host --example preview -- ~/dev/kits/kit-ci/tabs/ci.wasm`
   (`--light`, `--fr`), a pretend Hangar with two panes and every permission.

**Rules every port keeps** (kit-ui.md, "The rules a kit follows"): every string through
`i18n::Strings` in English and French, one primary action, the empty, loading, failed and denied
states drawn, `KitError` codes drawn rather than panicked on, no colour named but an ink on a canvas.

## What maps directly, for every kit

| In the page                                         | Natively                                                       |
| --------------------------------------------------- | -------------------------------------------------------------- |
| `hangar.call(method, params)`, every method used    | `cx.call(api::…, Message::…)`, or `cx.call_raw` — same names, params, errors |
| `hangar.on("panes.changed" / "tab.params" / "theme")`, `visibilitychange` | `Subscriptions::panes_changed`, `tab_params`, `theme`, `visibility` |
| `setInterval`, a re-armed `setTimeout` poll         | `Subscriptions::every(id, period, …)` (100 ms at least), a `polling` flag in the state |
| a one-shot `setTimeout`, a debounce                 | `cx.after(key, delay, …)`, `cx.cancel(key)`                    |
| `hangar.ui.i18n`, `ui.locale()`, `navigator.language` | `Strings`, `start.theme.locale`, the `theme` event            |
| `ui.format.duration`, `bytes`, `number`             | `format::duration`, `bytes`, `number`                          |
| `ui.format.since`, `clock`, `date`, `Date.parse`    | `format::since`, `clock`, `date` with `start.utc_offset`; `format::parse_iso` |
| `ui.toast`, `ui.confirm`, `ui.copy`, `copyButton`   | `cx.toast`, `cx.confirm`, `cx.copy` (from a click; a "Copied" toast after) |
| `ui.menu` built after a call, anchored to a button  | `menu([...]).id("hand").on_open(Message::Opened)`: list the items in the next view |
| `ui.log` (virtualised, errors, Previous/Next)       | `log_lines(…).id("job")`, `LogLine::current`, `cx.scroll_to_line("job", n)`; the count bar is the kit's own row |
| `kit-header`, a back button before the title        | `header(…).lead(icon_button(Icon::ArrowLeft, "Back").on_press(…))` |
| a rich row (several lines, a `pre`, banners, buttons under it) | `item(title).sub(…).body(…)`, `.mono()` for a path       |
| a section title in mono                             | a `section` whose children start with `text(path).mono()`, or rows with `.mono()` |
| `kit-empty` with a code line, a stderr `pre`        | `failed(…).icon(Icon::…).child(text("gh auth login").code()).child(pre(stderr))` |
| `kit-banner` with a bold heading, a switch in it    | `banner(…).title("Limits").action(switch(…))`                  |
| `select` with disabled options                      | `select_option(…).disabled(true)`, the reason in its label     |
| `input type=radio`                                  | `radio(options, Some(chosen)).on_select(…)`                    |
| an indeterminate `kit-progress`                     | `progress(None)` (it runs)                                     |
| a diff's added and removed lines                    | `LineKind::Added`, `LineKind::Removed`                         |
| keyboard shortcuts on the document or a field       | `Subscriptions::shortcut("mod+s", …)`                          |
| `element.focus()` after a redraw, a caret kept      | `text_input(…).id("note-3")`, `cx.focus("note-3")`; a caret survives views by itself |
| scroll kept across redraws                          | by itself, while the page keeps its shape                      |
| heights in `vh` (a log 56vh, a pre 24vh)            | a `page(…).fill()` with a `scroll` of `fill`, or a fixed `height` in px: no `vh` |

## The kits

Each kit: its tabs, what is direct, the gaps and what fills them, the traps.

### kit-template — `panes`

`panes.read`; a header, a row per pane with a glyph, empty, denied, error and loading states,
`panes.changed`. **Direct**: everything; it is `examples/kits/hello-native` almost line for line.
**Gaps**: none. Port its 9 tests; it is the template the others copy, so keep it minimal and
commented.

### kit-checklist — `checklist`

`files.read`, `data.read`, `data.write`, `panes.send`, `notify`; param `path`. A progress bar, a
group of steps with three filters (OK, Problem, Skip) and a note field, a foot with Send or Copy.
**Direct**: header with state, `progress(Some(..)).kind(…)`, filters, `text_area` per failed step,
`tab.setTitle`, `tab.setBadge`, `notify`, `panes.send`. **Gaps filled**: rows holding filters and a
note → `item(…).wrap().body(row![filter…]).body(text_area(...))`; the focus to the note on Problem →
`text_area("note-4", …)`, `cx.focus("note-4")`; the debounced save (1 500 ms, never closer than
7 000 ms, a retry after 15 000 ms on `rate_limited`) → `cx.after("save", …)`; the copy button →
`cx.copy(summary)` and a toast. **Cannot**: flush on `pagehide`: a closing tab gets no last update;
the debounce loses at most 1.5 s of answers (kits-native.md, "Later").

### kit-annotate — `annotate`

`files.read`, `files.write`, `panes.send`; param `path`. A capture drawn on with an arrow, a
rectangle, a freehand stroke and numbered notes, in five colours, zoomable; written as a PNG and a
JSON beside it, then told to an agent. **Direct**: the header, the tool filters, the notes section of
text inputs, the comment and "Send to" fields, the foot, `ui.confirm` before dropping marks,
`files.write` of the JSON, `panes.send`. **Gaps filled**:

- The capture → `canvas("shot", w, h).background(ImageSource::File(path))`: shown where it is, never
  read into the module (`files.readBinary` of a 10 MB capture would not fit a 4 MB message). Its
  size first: `cx.call(api::ImageSize { source: ImageSource::File(path) }, Message::Sized)` (or the
  `width`, `height` of `ui.pasteImage`'s answer); draw `loading(…)` until it comes. The canvas's
  units are then the capture's pixels, as the page's marks are.
- The tools → `.tool(Some(Tool::Arrow | Rect | Free | Point))`, the colour → `.ink(Ink::Red…)`, the
  swatches → `swatch(Ink::Red, "Red").selected(…)`; the five colours are `Ink::Red`, `Orange`,
  `Green`, `Blue`, `Magenta`, kit-annotate's own values.
- A drag → `on_draw(Message::Drew)`; drop what `model.fromDrag` drops (an arrow under 4 units, a
  rectangle under 2 a side) in `update`. A note → the `point` tool and `on_point`, then
  `Mark::circle(at, 3.4 * line).fill(ink)`, `Mark::text(at, n, 4.25 * line).bold().align(Center)
  .ink(ink.contrast())`, and the label `Mark::text((x + r + pad, y), label, 3.57 * line)
  .fill(ink).ink(ink.contrast()).fallback((x - r - pad, y))`, with `line = default_stroke(w, h)`.
- Zoom → `.zoom(None)` for Fit (`on_fit` gives its scale for the label), `.zoom(Some(z))` inside
  `scroll(…).axis(Axis::Both)` past it; the steps of `logic.stepZoom` stay the kit's.
- Undo on Ctrl+Z → `shortcut("mod+z", Undo)`. It also comes while a note's field has the focus
  (iced's fields have no undo of their own), which the page ignored: the kit keeps which note was
  typed in last, set by the field's `on_input` and cleared by a press on the canvas, and ignores the
  shortcut while one is.
- Export → `cx.call(api::CanvasExport::of(&canvas, ".hangar/annotations/20251031-211456.png"),
  …)`: Hangar draws it, scales it down to fit 10 MB, writes it new (`conflict` → the next name, as
  `logic.names` does). Its answer's `width`, `height` and `scale` go in the JSON.
- The file name's local time (`logic.stamp`) → `format::local_time(cx.now(), cx.utc_offset())`,
  its `year`, `month`, `day`, `hour`, `minute`, `second`.
- "Another image" → `files.pick { extensions: [png, jpg, jpeg, webp, gif] }` from the click: a file
  of the workspace comes back as `path`, one elsewhere as `held` (`ImageSource::Held`), which
  `canvas.export` draws on. Paste → `shortcut("mod+v", Paste)` → `cx.call(api::PasteImage {}, …)`.
- Focus to the new note's field → `cx.focus("note-n")`.

**Cannot**: a file dropped on the tab (the window hears drops, not where they land) — the picker
replaces it; selecting or moving a mark after it is drawn, which the page does not do either.

### kit-editor — `editor`

`files.read`, `files.write`, `panes.send`; params `path`, `keys`, `why`, `line`. A file picker, a
dotenv form, a text editor with a gutter, a diff review, a conflict view. **Direct**: `files.list`
grouped in sections of rows (`.mono()` titles), `search`, banners, `files.read`/`files.write` with
`expected` and `conflict`, `tab.setTitle`, `tab.setBadge`, `panes.send` of answer.js's text, the
leave-with-edits `ui.confirm`. **Gaps filled**:

- The code editor → `text_area("text", &text).line_numbers().fill().mark(line_mark(n,
  Notice::Info))` for the asked line and `Notice::Error` for the check's line (a `label` on hover);
  `cx.cursor_to("text", line, 1)` jumps there and scrolls; `.fill()` in a `page(…).fill()`.
- Ctrl+S → `shortcut("mod+s", Save)`: in a value field it saves, in the editor it reviews; the kit
  knows which view it is in.
- The diff → `log_lines(…)` with `LineKind::Added` / `Removed` / `Head`, numbered.
- Yours / On disk → `radio([…], Some(..)).horizontal()`.
- "I know this file is broken" → `banner(Notice::Error, …).action(switch(…))`.
- Secret values → `text_input("", &value).secure()` with a Show/Hide button toggling `secure`, per
  row, hidden again after a save; a multi-line value shown as a secure one-line field until revealed,
  then a `text_area` of 8 lines. Never `console::*` a value; answer.js's `mayName` rules port as they
  are, and so do the 400-file fuzz and its "no value reaches an agent" tests.

**Cannot**: a secure multi-line field (iced's editor has no mask): the one-line placeholder stands
in, as the page's does. Long lines in the numbered editor are cut at its edge, not scrolled to.

### kit-ci (`ci-status`) — `ci`

`command:ci-run`, `command:ci-list`, `command:ci-log`, `notify`; param `id`. A run's jobs with live
durations, the failed log, a picker of runs. **Direct**: `command.run` of the three commands,
`tab.setTitle`, `tab.setBadge`, `notify` when a run ends, the 5 s poll and 1 s clock (timers), rows
with glyphs. **Gaps filled**: the back icon → `header(…).lead(icon_button(Icon::ArrowLeft, …))`;
the failed log (1 MB, tens of thousands of lines) → `log_lines(…).id("log")`, the kit's
`isErrorLine` setting `LineKind::Error`, "N errors · error i of N" with Previous/Next as the kit's
own row, `LogLine::current` and `cx.scroll_to_line("log", i)`; to keep views small, give it in
parts: `.window(first, total).on_range(Message::Lines)` and keep the lines in the state; ages and
durations → `format::since`, `format::duration`; `Date.parse` and GitHub's `0001-01-01` →
`format::parse_iso`; the copyable code lines → `text(…).code()` and a copy icon button
(`cx.copy`). **Note**: kit.js timed calls out with `timeout` after 30 s (130 s for `command.run`);
natively only the command's own `timedOut` remains: `classify` loses that branch.

### kit-db — `db`

`command:list`, `command:tables`, `command:schema`, `command:rows`, `files.read`, `panes.read`,
`panes.send`, `data.read`, `data.write`; param `path`; singleton. **Direct**: the four commands
(the Node tools stay), the table list, `kit-tabs` → `tabs(…)`, structure as a grouped list and
chips, paging with Previous/Next, `panes.send`. **Gaps filled**: the grid → `table(columns, rows)`
with `.on_sort(…)` (server-side sort; a third press on the descending column means none, read in
`update`), `height(Length::Px(…))`; cells cut with `text(…).truncate()` and the whole value on
hover (`tooltip`); "Show all N characters" → a row press (`on_row`) opening a `pre` under the table;
unusable files → `select_option(…).disabled(true)`. **Traps**: never log or `panes.send` a cell
value (they may be secrets; the tests plant some). **Bug to fix while porting**: controller.js:213
reads `event.data.params.path`, but the `tab.params` event's data is `{ params, caller }` itself, so
a singleton's new path is ignored today; natively `Subscriptions::tab_params` hands the right shape.
`files.read` is declared and never used: keep it (the person accepted it), or drop it with a
version bump.

### kit-deps — `deps`

`command:audit`, `panes.read`, `panes.send`, `data.read`, `data.write`. **Direct**: the audit on a
click, `last.json` in the data folder, sections of vulnerabilities and outdated packages, "Show N
more", a `select` of agents, `panes.send`, `tab.setTitle`, `tab.setBadge`, the 30 s clock.
**Gaps filled**: the running bar → `progress(None)`; rows with a meta line, a banner and a button
under them → `item(…).body(…)`; "Last run 5 min ago" → `format::since`. **Bug to fix while
porting**: controller.js:146-151 writes the clipped result (no `schema`, no `counts`) and
logic.js:203 requires `schema === 1`, so the last audit never reloads; write what `parseResult`
reads, and test the round trip.

### kit-github — `pulls`, `issues`

`command:repo-info`, `pr-list`/`pr-view`/`pr-threads` (pulls) or `issue-list`/`issue-view`
(issues), `panes.read`, `panes.send`, `data.read`, `data.write`; both singletons. One crate, one
module for both tabs (`start.tab.id`). **Direct**: the commands, filters with counts, the search,
the detail's key/value list and sections, `tab.setBadge`, the filters saved after 700 ms
(`cx.after`), the 60 s refresh only while visible (`visibility`), the 5 s gap on return.
**Gaps filled**: "Hand to an agent" built after `panes.list` → `menu(items).id(format!("hand-{n}"))
.on_open(Message::Panes)`: call `panes.list` on open, list them as they come; the row's action line
→ `item(…).sub(…).body(text(second line).small().dim()).body(row![button("Hand…")])`; "Updated 3
min ago" → `format::since`. The description stays raw Markdown in a `pre`, as the page shows it.
**Keep**: the `<<<GITHUB-TEXT>>>` quoting and the control/bidi stripping of logic.js, ported with
their tests.

### kit-guard — `guard`

`data.read`, `data.write`. **Direct**: the 2 s poll of `refusals.jsonl` and `errors.jsonl`, rows,
"Allow once" writing `passes/<key>.json`, `state.json`, `tab.setBadge`. **Gaps filled**: the
countdown and times → a 1 s or 2 s timer and `format::since`, `format::date` for the tooltip; the
limits banner's bold heading → `banner(…).title("…")`; rows with a `pre` and a banner →
`item(…).body(pre(command).height(160.0)).body(banner(…))`. **Note**: the JSONL can reach 512 KB of
several hundred rows: all drawn is fine; cap the `pre`s' height as the page does.

### kit-services — `services`

`command:ports`, `command:docker-list`, `command:docker-logs`, `panes.read`, `panes.send`;
singleton. **Direct**: the three commands in parallel every 10 s while visible (`visibility`), the
filters, the rows, the docker logs (200 lines), `panes.send`. **Gaps filled**: the ask panel and
the logs under a row → `item(…).body(…)` (the kit keeps which is open); the logs → `log_lines(…)
.id(format!("logs-{id}")).height(320.0)`; "Updated 21:14:56" → `format::clock(now, offset, true)`;
"started 5 min ago", also in the brief to the agent → `format::since`; `Date.parse(startedAt)` →
`format::parse_iso`. The keyed partial redraw of view.js has no equivalent to port: a view is drawn
whole and keeps its fields' state.

### kit-tests — `tests`

`data.read`, `panes.read`, `panes.send`. **Direct**: the 3 s poll of `index.json` and the runs that
changed, sections per project, `panes.send` of the brief. **Gaps filled**: rows with a `pre`, a
banner and a button → `item(…).body(…)`; `format::since`, `format::date` (tooltip),
`format::duration`. **Trap**: 40 runs × 50 failures × 2 000 characters is about 4 MB, the most a
view may be: show each run's failures folded past the first five ("Show N more"), so a view stays
far under it.

### kit-todo — `todo`

`command:scan`, `panes.read`, `panes.send`, `preview.show`, `data.read`, `data.write`. A list of
TODO/FIXME/HACK/XXX comments (not a todo list). **Direct**: the scan, `last.json`, the tag filters,
the group-by `select`, per-file "Who wrote it" (`scan --blame`), Open (`preview.show`), Hand to an
agent, "Show 100 more", the 30 s clock. **Gaps filled**: the running bar → `progress(None)`; the
search → `search(…).id("q")`, its caret kept by itself; rows with a chip, a mono location, an author
line and two buttons → `item(…).body(…)`; mono section titles → a mono text first in the section;
`format::since`. **Trap**: `last.json` can pass the 1 MB of `data.write` with 1 000 markers: the
page fails silently; keep the markers it shows, and say when it was not saved.

## What a native tab cannot do, and why

- **A file dropped on the tab**: the window hears a drop and its path, not where it lands. The
  picker (`files.pick`) and the paste (`ui.pasteImage`) stand in. (kit-annotate)
- **Select text with the pointer**: iced's text cannot be selected. Copy buttons (`cx.copy`) and a
  read-only `text_area` stand in. (every kit that shows commands and logs)
- **A last write when the tab closes**: the instance is dropped at once; saving as it goes, debounced,
  loses at most the debounce. (kit-checklist)
- **A secure multi-line field**: iced's editor has no mask. (kit-editor)
- **Wrapped lines in a long log**, a numbered editor that scrolls sideways: lines of one height are
  what make tens of thousands of them cheap. (kit-ci, kit-editor)
- **Sizes relative to the window (`vh`)**: a page that fills and a `scroll` of `fill`, or pixels.
- **Accessibility roles** (`aria-*`, `role=alert`): iced has no accessibility tree; tooltips name
  icons and glyphs.
- **A client-side timeout on calls**: kit.js's 30 s; only a command's own limit remains.

Everything else the twelve tabs do has a native widget, call or subscription above.
