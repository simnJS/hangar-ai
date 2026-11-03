# Native kit tabs

A kit's tab can be **native**: Rust compiled to WebAssembly, describing its page as a tree of
widgets that Hangar draws itself, with its own widgets, in the person's theme. No browser engine
runs for it. This page is the contract, for kit authors and for Hangar.

What does not change: everything else a kit is ([kits.md](../kits.md)) — its manifest, its plugin,
its setup commands, its data folder, its declared commands, the permissions the person accepts.
A native tab calls the same methods an HTML tab calls, under the same permissions and limits, and
hears the same events. An **HTML tab keeps working**, in the separate `hangar-webview` helper
process, for a kit that depends on the web.

| Part                                         | Where                                                        |
| -------------------------------------------- | ------------------------------------------------------------ |
| The data: widgets, events, ABI, host API     | `crates/hangar-kit-ui`, public contract, serde only          |
| The kit's side: `Kit`, `export_kit!`, builders, a test harness | `crates/hangar-kit-sdk`, builds for `wasm32-unknown-unknown` and the host |
| Hangar's side: the runtime, the gate, the drawing | `crates/hangar-kit-host`, wasmtime 50 and iced 0.14     |
| An example kit                               | `examples/kits/hello-native`                                 |
| A kit to try a module in a window            | `cargo run -p hangar-kit-host --example preview -- tab.wasm` |

## In short

```text
module exports  memory, hangar_abi_version() -> i32, hangar_alloc(len) -> ptr,
                hangar_init(ptr, len) -> i64, hangar_update(ptr, len) -> i64, hangar_view() -> i64
module imports  hangar.log(level, ptr, len), hangar.now() -> i64; nothing else, no WASI
messages        UTF-8 JSON: Start and Input in, Output and View out; an i64 answer is ptr << 32 | len
ownership       Hangar writes into what hangar_alloc gave, the module frees it; the module's answer
                stays the module's until the next call, Hangar copies it first
loop            init -> view, then for each input: update -> (calls, subscriptions) -> view
```

Why WebAssembly and not a native library: a Rust dylib has no stable ABI across compiler versions,
and runs with every right of Hangar's process. A WebAssembly module has a stable ABI that is ours
to define, and runs in a sandbox that reaches nothing it is not handed.

## The manifest

A tab is native when its `entry` names a `.wasm` file. Everything else about the tab is as
[kits.md](../kits.md#the-manifest-hangar-kitjson) says: `id`, `title`, `icon`, `permissions`,
`params`, `singleton`.

```json
{
  "schema": 2,
  "id": "hello-native",
  "name": "Hello native",
  "version": "0.1.0",
  "description": "A native tab written in Rust.",
  "hangar": ">=2.0.0",
  "tabs": [
    {
      "id": "hello",
      "title": "Hello",
      "entry": "tabs/hello.wasm",
      "permissions": ["panes.read", "panes.send", "data.read", "data.write"]
    }
  ]
}
```

**Schema 2.** A tab whose `entry` ends in `.wasm` needs `"schema": 2`. A manifest of schema 1 with
such an entry is a manifest error: `tabs[0].entry: a native tab needs "schema": 2`. Schema 2 changes
nothing else: a Hangar that reads it reads schema 1 as it always did.

**On an older Hangar.** One that reads schema 1 only says, as it always has for a higher number,
`schema: 2 needs a newer Hangar`, and the kit is unusable there, plugin included: the person is told
why, rather than opening a tab that cannot run. A kit says what it needs the usual way too,
`"hangar": ">=2.0.0"`. The native Hangar is taken to ship as 2.0.0; if it ships under another
number, only that number changes here.

**Both, for a kit that must still open on 1.x.** Stay at schema 1, keep an HTML `entry`, and add a
`native` field to the tab:

```json
{ "id": "ci", "title": "CI", "entry": "tabs/ci/index.html", "native": "tabs/ci.wasm", "permissions": ["command:ci-run"] }
```

An older Hangar ignores the field, as it ignores every field it does not know, and opens the page.
A native Hangar opens the module, and the page only when the module cannot be loaded (an ABI it does
not read, a module it refuses): then it opens the page in the webview helper, and says so in the
tab's bar. `native` must name an existing `.wasm` file inside the kit; in schema 2 it is accepted
beside an HTML `entry` with the same meaning.

**Where the module comes from.** Hangar never builds a kit: it runs the `.wasm` the kit ships. A kit
on GitHub commits its built module, or the release tag it is pinned to holds it. The kit scheme's
rules hold for it as for any file of the kit: inside the kit's folder, not hidden, 64 MB at most.

**Hangar's part.** `crates/hangar-core/src/kits/manifest.rs` reads schemas 1 and 2 with these
rules: `Tab::module()` is the file to run, `Tab::page()` the HTML the kit scheme serves, and the
kit scheme refuses a `.wasm` entry as a page (types in `docs/native/core-api.md`).
`hangar_kit_host::KitRuntime::compile` is what the app calls on the file the manifest names, and
the fallback to the page when it fails is the app's.

## Building a tab

A tab is a library crate built as a `cdylib`, depending on `hangar-kit-sdk`:

```toml
[package]
name = "my-tab"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib", "rlib"]   # the module, and the library its tests use

[dependencies]
hangar-kit-sdk = { git = "https://github.com/hangar-ai/hangar", tag = "v2.0.0" }

[profile.release]
opt-level = "s"   # smaller modules load faster; none of this is required
lto = true
strip = true
```

```sh
rustup target add wasm32-unknown-unknown            # once
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/my_tab.wasm tabs/my.wasm
```

**`wasm32-unknown-unknown`, not `wasm32-wasip1`**, and no `wasm-bindgen`: Hangar links no WASI and no
JavaScript glue, and refuses a module that imports either, saying which target to build for. Rust's
standard library works there (collections, strings, formatting, serde); what needs the system does
not: no clock (use `cx.now()`), no file, no thread, no network. Those go through Hangar.

A module of the example is about 500 KB in release, most of it serde_json and formatting. That is
compiled once per file and shared by every tab opened from it.

**Testing a tab without Hangar.** `hangar_kit_sdk::testing::Harness` runs the same runtime the module
runs, on the host, with no WebAssembly: start it, answer its calls by hand, press its buttons, read
its page.

```rust
let mut tab = Harness::<Hello>::new(testing::start().grant(["panes.read"]));
tab.reply_to("panes.list", Ok(json!([{ "name": "Ava", "agent": "claude", "role": null, "state": "idle", "canSend": true }])));
assert!(tab.shows("1 pane"));
tab.press("Refresh");
assert_eq!(tab.methods(), vec!["panes.list"]);
```

`cargo test` runs these, since the crate is also an `rlib` and `export_kit!` writes nothing off
wasm32. **In a window**, `cargo run -p hangar-kit-host --example preview -- tabs/my.wasm` opens the
module in a pretend Hangar (two panes, a data folder in memory, every permission granted), 360 points
wide, `--light` and `--fr` for the other theme and language. **In Hangar**, link the kit's folder
(read live) and reload the tab from its bar after each build.

**In this repository** the example's tab (`examples/kits/hello-native/tab`) and the host's test kit
(`crates/hangar-kit-host/tests/probe`) are members of the workspace: they build for the host as well,
so the workspace's checks reach them, and they share its lockfile, which the host's tests need to
build them for wasm32 without touching the network. `examples/kits/hello-native/build.sh` builds the
example and puts the module where its manifest says.

## A tab's life

A tab is a `Kit`, the shape of an iced program: state, messages, `update`, `view`, `subscriptions`.

```rust
use hangar_kit_sdk::prelude::*;

struct Panes { panes: Option<Result<Vec<api::Pane>, KitError>> }

#[derive(Clone)]
enum Message { Refresh, Panes(Result<Vec<api::Pane>, KitError>) }

impl Kit for Panes {
    type Message = Message;

    fn init(_start: &Start, cx: &mut Context<Message>) -> Self {
        cx.call(api::PanesList {}, Message::Panes);
        Panes { panes: None }
    }

    fn update(&mut self, message: Message, cx: &mut Context<Message>) {
        match message {
            Message::Refresh => cx.call(api::PanesList {}, Message::Panes),
            Message::Panes(panes) => self.panes = Some(panes),
        }
    }

    fn view(&self) -> Element<Message> {
        let body: Element<Message> = match &self.panes {
            None => loading("Reading the panes…").into(),
            Some(Err(error)) if error.is(KitError::DENIED) => denied("This tab may not list the panes").into(),
            Some(Err(error)) => failed(error.message.clone()).into(),
            Some(Ok(panes)) => list(panes.iter().map(|pane| item(pane.name.clone()))).into(),
        };
        page![header("Panes").action(button("Refresh").on_press(Message::Refresh)), body].into()
    }

    fn subscriptions(&self) -> Subscriptions<Message> {
        Subscriptions::none().panes_changed(Message::Refresh)
    }
}

export_kit!(Panes);
```

1. **Start.** Hangar instantiates the module, one instance per open tab, asks
   `hangar_abi_version`, then calls `hangar_init` with a `Start`: the ABI, Hangar's version, the kit,
   the tab with its params and caller, the theme's scheme and language, the permissions the tab holds
   now, and whether it is in front. `init` answers its first calls and subscriptions; Hangar then
   asks for the page (`hangar_view`).
2. **Every input** — a widget used, a call answered, a timer, an event — is one `hangar_update`.
   It answers new calls and the subscriptions as they are now; Hangar asks for the page again.
3. **Calls are tasks.** Nothing a kit asks for happens while `update` runs: the call goes out when
   it returns, and its answer comes back later as an update of its own, the message `on_reply` made
   of it. A module never waits on Hangar, so a call that takes two minutes (`command.run`) holds
   nothing.
4. **Subscriptions are declared, all of them, after every update**: timers by id and period, and
   Hangar's events by name. What the kit leaves out stops; a timer that keeps its id and period keeps
   running. A timer runs at 100 ms at the shortest, 16 per tab at most.
5. **It ends** when the tab is closed, or its workspace is put to sleep: the instance is dropped,
   with its memory. A kit keeps what must outlive it in its data folder (`data.write`). Restarting
   Hangar brings the tab back with its params, and it starts from `init` again.

**Handlers and views.** A widget names its handler by a number that the view gives it; the SDK keeps
the handlers of each view, and the event that comes back names the number and the view (`seq`) it
was drawn from. Hangar may draw a new view between a click and its delivery: the SDK keeps the last
eight views' handlers, so the click lands on the handler it was drawn with, as an iced message would.

**Events** a tab can subscribe to:

| Event            | Needs        | `data`                                      | When                                                   |
| ---------------- | ------------ | ------------------------------------------- | ------------------------------------------------------ |
| `panes.changed`  | `panes.read` | `{}`                                        | The workspace's panes change; call `panes.list`.        |
| `tab.params`     | —            | `{ params, caller }`                        | A singleton got new params, or the tab was asked for again. |
| `theme`          | —            | `{ scheme, locale }`                        | The workspace's theme or Hangar's language changed.     |
| `tab.visibility` | —            | `{ visible }`                               | The tab came to the front or went behind. New: a native tab has no `document.hidden`. |

The `theme` event carries no tokens: Hangar draws the widgets, so a kit has nothing to apply. The
scheme is for a kit's own images, the language for its strings. An event the tab may not hear is
left out silently, as for an HTML tab, the grant being read again at each event.

## The WebAssembly ABI

`hangar_kit_ui::abi` is this section in Rust. Any language that builds a WebAssembly module with
these exports and speaks this JSON can write a tab; the SDK is the Rust way to.

**Exports.**

| Export               | Type                  | Does                                                                  |
| -------------------- | --------------------- | --------------------------------------------------------------------- |
| `memory`             | memory                | The module's linear memory, one, 32-bit.                              |
| `hangar_abi_version` | `() -> i32`           | The ABI it was built against: `1`. Called first.                      |
| `hangar_alloc`       | `(len: i32) -> i32`   | A buffer of `len` bytes for Hangar to write a message into.           |
| `hangar_init`        | `(ptr, len) -> i64`   | A `Start` in, an `Output` out. Once per instance.                     |
| `hangar_update`      | `(ptr, len) -> i64`   | An `Input` in, an `Output` out.                                       |
| `hangar_view`        | `() -> i64`           | The `View` of the state as it is now.                                 |

**Imports**, from the module `hangar`, and nothing else:

| Import       | Type                          | Does                                                            |
| ------------ | ----------------------------- | --------------------------------------------------------------- |
| `hangar.log` | `(level: i32, ptr: i32, len: i32)` | A line for the tab's log: 0 debug, 1 info, 2 warn, 3 error, 4 panic. 4 KB kept per line. |
| `hangar.now` | `() -> i64`                   | Milliseconds since the Unix epoch, UTC.                          |

**Who owns the bytes.**

- Into the module: Hangar calls `hangar_alloc(len)`, writes the message at the pointer it answers,
  and passes `(ptr, len)` to `hangar_init` or `hangar_update`. The buffer is the module's from then
  on: it reads it and frees it.
- Out of the module: an export answers `ptr << 32 | len` (an `i64`), a buffer the module keeps.
  Hangar copies it before it calls the module again, and the module may free or reuse it at its next
  call. The SDK keeps the last answer in one buffer, replaced at each call.
- `hangar.log` reads the module's memory during the call only.
- A pointer and a length outside the module's memory stop the tab (`Message`), as does a message of
  more than 4 MB either way.

**The messages**, UTF-8 JSON, camelCase as the tab message protocol is:

```jsonc
// Start, to hangar_init
{ "abi": 1, "hangar": "2.0.0",
  "kit": { "id": "hello-native", "name": "Hello native", "version": "0.1.0" },
  "tab": { "id": "hello", "params": { "run": "4567" }, "caller": { "pane": "Ava" } },
  "theme": { "scheme": "dark", "locale": "fr" },
  "permissions": ["panes.read", "data.read"], "visible": true }

// Input, to hangar_update: one of
{ "type": "ui", "view": 7, "handler": 3, "value": { "text": "owner/name" } }
{ "type": "reply", "id": 12, "result": [ … ] }
{ "type": "reply", "id": 13, "error": { "code": "denied", "message": "this tab was not granted panes.send" } }
{ "type": "timer", "id": "poll" }
{ "type": "event", "name": "panes.changed", "data": {} }

// Output, from hangar_init and hangar_update
{ "calls": [ { "id": 12, "method": "panes.list", "params": null } ],
  "subscriptions": { "events": ["panes.changed"], "timers": [ { "id": "poll", "everyMs": 1000 } ] } }

// View, from hangar_view
{ "seq": 7, "root": { "type": "page", "children": [ { "type": "header", "title": "Hello" }, … ] } }
```

A call's `method` and `params` are those of an HTML tab's `hangar.call(method, params)`, and its
answer is the same `result` or `error`. A call `id` is the module's own, unique among its calls still
waiting.

**Why JSON.** It is what the protocol of HTML tabs already is, so the methods, params, results and
errors are the same text in both, and one set of rules checks both. It can be read: Hangar can log a
tab's traffic, and a kit author can print a view and see their page. Every language that compiles to
WebAssembly has a JSON library. And it evolves the way kits need: a field added with a default, a
widget, a method, an event or an input kind are ignored by an older side, where a binary encoding
(bincode, postcard) breaks on any change of shape. What it costs is measured in microseconds: a view
of a thousand widgets is about 100 KB and parses in well under a millisecond, and it is sent once per
update, not per frame. If that ever matters, the envelope can carry another encoding under a new ABI.

**Versions.** `hangar_abi_version` answers `1`. The number changes only when an export or an import
changes its signature or its meaning, or the envelope does; Hangar refuses a module of an ABI it does
not read, saying whether Hangar or the SDK is the older. A new widget, field, method, event or input
kind changes nothing: a module ignores an input `type` it does not know (the SDK does), and Hangar
draws a widget `type` it does not know as a line saying a newer Hangar is needed, and the rest of the
page as it is. What a kit needs of Hangar beyond the ABI, it says with `hangar` in its manifest.

## The widgets

`hangar_kit_ui::Element` is the tree. Every widget is an object whose `type` names it, every field
with a default may be left out, and a kit never names a colour: it says what something is (`dim`,
`danger`, a `title`), and Hangar draws it in Hangar UI's tokens, in the person's theme. The
[rules a kit follows](../kit-ui.md#the-rules-a-kit-follows) hold for a native tab as they do for a
page: one primary action, designed empty, loading, failed and denied states, numbers at the end.

**Every component of Hangar UI** ([kit-ui.md](../kit-ui.md)), natively:

| Hangar UI                                    | Native widget (`type`)                         | SDK                                  |
| -------------------------------------------- | ---------------------------------------------- | ------------------------------------ |
| `.kit-page`                                  | `page`                                         | `page![…]`                           |
| `.kit-fill`, `.kit-grow`                     | `page` with `fill`, and a `scroll`             | `page(…).fill()`, `scroll(…)`        |
| `.kit-stack`, `.kit-actions`, `.kit-bar`     | `column`, `row` (`wrap`), `bar` (`foot`)       | `col![…]`, `row![…].wrap()`, `bar(…)` |
| Text classes: `.kit-dim` … `.kit-code`       | `text` with `size`, `tone`, `weight`, `mono`, `numeric`, `code`, `wrap: truncate` | `text("…").dim().mono()` |
| `.kit-sr`                                    | — (iced has no accessibility tree yet: see Later) |                                   |
| `.kit-header`                                | `header` (`title`, `state`, `actions`)         | `header("…").state(…).action(…)`     |
| `.kit-section`                               | `section` (`title`, `count`, `actions`)        | `section("…", […])`                  |
| `.kit-rows`, `.kit-row`, `--group`           | `list` of `ListItem` (`lead`, `title`, `sub`, `end`, `selected`, `wrap`), `group` | `list(…)`, `item("…")` |
| `.kit-btn` and its kinds, `--small`, `--icon`, `disabled` | `button` (`kind`, `small`, `icon`, no handler = disabled) | `button("…").primary()`, `icon_button(…)` |
| `.kit-icon`                                  | `icon` (12 icons, a `tone`, a `size`)          | `icon(Icon::Alert)`                  |
| `.kit-field`, `.kit-input`                   | `textInput` (`label`, `hint`, `error`, `secure`, `mono`) | `text_input("…", &value)`  |
| `textarea.kit-input`                         | `textArea` (`id`, `height`)                    | `text_area("id", &value)`            |
| `.kit-select`                                | `select` (`options`, `selected`)               | `select(…, Some(…))`                 |
| `.kit-search`                                | `search` (and its clear button)                | `search("…", &value).on_clear(…)`    |
| `.kit-switch`                                | `switch`, and `checkbox`                       | `switch("…", on)`, `checkbox(…)`     |
| `.kit-chips`, `.kit-chip`                    | a wrapping `row` of `chip`; with a `tone`, a badge | `chip("…").tone(Tone::Ok)`       |
| `.kit-filter`                                | `filter` (`pressed`, `count`, `countTone`)     | `filter("…", pressed)`               |
| `.kit-dot`, `.kit-glyph`, `.kit-state`       | `glyph` (`dot`), `state`                       | `glyph(…)`, `state_label(…)`         |
| `.kit-progress` and its states               | `progress` (`value`, none to say indeterminate; `kind`) | `progress(Some(0.4))`        |
| `.kit-banner`                                | `banner` (`kind`, `actions`)                   | `banner(Notice::Warning, "…")`       |
| `.kit-empty` (empty, loading, error, denied) | `empty` (`kind`, `title`, `text`, `actions`)   | `empty`, `loading`, `failed`, `denied` |
| `.kit-pre`                                   | `pre` (`height`)                               | `pre("…")`                           |
| `.kit-log`, `hangar.ui.log`                  | `log` of `LogLine` (`kind`, `number`, `current`), `atEnd` | `log_lines(…)`, `log_line(…)` |
| `.kit-kv`                                    | `keyValue`                                     | `key_value([("Branch", "main")])`    |
| `.kit-tabs`, `hangar.ui.tabs`                | `tabs` (the kit draws the chosen panel)        | `tabs(…, "jobs")`                    |
| `title` (tooltip)                            | `tooltip`, and `tooltip` on buttons and rows   | `tooltip(child, "…")`                |
| `.kit-menu`, `hangar.ui.menu`                | `menu` of `MenuItem` (`hint`, `danger`, `disabled`, `checked`) and separators | `menu([…])` |
| `.kit-confirm`, `hangar.ui.confirm`          | the call `ui.confirm`                          | `cx.confirm(…)`                      |
| `.kit-toast`, `hangar.ui.toast`              | the call `ui.toast`                            | `cx.toast(…)`, or `cx.call(api::Toast …)` |
| `hangar.ui.copy`, `copyButton`               | the call `ui.copy`, from a click               | `cx.copy("…")`                       |
| `hangar.ui.list` (a long list)               | `list`, `log` or `table` with `height`, given the lines in view: see Later | |
| `hangar.ui.i18n`, `locale`                   | `hangar_kit_sdk::i18n::Strings`, `start.theme.locale` and the `theme` event | `Strings::new(…).add("fr", …)` |
| `hangar.ui.format` (duration, bytes, number) | `hangar_kit_sdk::format`                       | `format::duration(ms, locale)`       |
| `format.clock`, `date`, `since`              | — (no time zone in wasm32: see Later)          |                                      |
| Focus ring                                   | Hangar's own, on every control                 |                                      |

**What a native tab adds:**

| Widget        | What                                                                        |
| ------------- | --------------------------------------------------------------------------- |
| `grid`        | Children in `columns`, or as many of `minWidth` as fit.                     |
| `scroll`      | The part that scrolls, `vertical`, `horizontal` or `both`; `stickToEnd`.    |
| `split`       | Two parts side by side or one above the other, at a `ratio`.                |
| `container`   | One child on a `surface` (`card`, `raised`, `float`, `field`, `well`, `selected`, `accent`, `danger`), `border`, `radius`, `padding`, alignment. |
| `space`, `divider` | Room, and a hairline.                                                  |
| `spans`       | Text in several styles on the same lines; a span with a handler is a link.  |
| `link`        | Text in the accent's colour that acts like a button.                        |
| `table`       | Columns (`align`, `width`, `sortable`), rows of cells (any widget, a sort `key`), `selected`, `height`. |
| `tree`        | Nodes that open and close, with icons; a node chosen.                       |
| `image`       | From the kit's folder (`{ "kit": "assets/x.png" }`, no permission) or its data folder (`{ "data": … }`, needs `data.read`); PNG, JPEG, GIF, WebP, SVG, 10 MB; `fit`, a `description`. |
| `sparkline`   | A line of values, small, `fill` under it.                                   |
| `bars`        | Labelled bars, across or standing.                                          |

Sizes are logical pixels: `{ "px": 120 }`, `"fill"`, `{ "portion": 2 }`, `"shrink"`. Hangar's
interface size scales the whole window, so a kit never multiplies by it. Spacing follows Hangar UI's
steps (2, 4, 6, 8, 10, 12, 16, 20, 24). A `row` and a `column` fill their width by default, so a
`space` of `fill` width pushes what follows to the end.

**What each handler sends back** (`UiEvent.value`):

| Handler                                                         | Value                          |
| --------------------------------------------------------------- | ------------------------------ |
| `onPress` of a button, link, list item, filter, span; `onSubmit`, `onClear` | none               |
| `onInput` of a text input, text area, search                    | `{ "text": the whole text }`   |
| `onToggle` of a checkbox, switch                                | `{ "bool": the new state }`    |
| `onSelect` of a select (option's value), tabs, menu, tree (ids) | `{ "text": … }`                |
| `onRow` of a table                                              | `{ "index": its index in the rows given }` |
| `onSort` of a table                                             | `{ "sort": { "column", "descending" } }` |

Inputs are **controlled**: a text input shows the `value` the kit gives, and the kit gives the new one
back in its next view, as in iced.

**What Hangar keeps itself**, so that a kit does not have to round-trip for it: a table's order when it
has no `onSort` (by the cells' keys, numbers before texts, texts without regard to case, what has no
key last, kept under the table's `id`); which tree nodes are open (from each node's `open`, kept under
the tree's `id`); a text area's cursor and selection (kept under its `id`, its text replaced when the
kit gives another than the one typed). A text area without `onInput` is read-only.

## The host API

The methods of the [tab message protocol](../kits.md#the-tab-message-protocol), by the same names,
with the same params, results, permissions, limits and error codes. `hangar_kit_ui::api` has them
typed (`api::PanesList`, `api::FilesWrite`…), each with its `METHOD` and the type it answers.

| Method              | Params                                       | Needs                     | Result |
| ------------------- | -------------------------------------------- | ------------------------- | ------ |
| `workspace.info`    | —                                            | `workspace.read`          | `{ id, name, folder }` |
| `panes.list`        | —                                            | `panes.read`              | `[{ name, agent, role, state, canSend }]` |
| `panes.send`        | `{ pane, text }`                             | `panes.send`              | `{ queued: true, pane }` |
| `files.read`        | `{ path }`                                   | `files.read`              | the text |
| `files.readBinary`  | `{ path }`                                   | `files.read`              | `{ base64, mime }` |
| `files.list`        | `{ path?, pattern? }`                        | `files.read`              | `{ files: [{ path, size, modified }], truncated }` |
| `files.write`       | `{ path, text, expected? }`                  | `files.write`             | `{}` |
| `files.writeBinary` | `{ path, base64 }`                           | `files.write`             | `{}` |
| `data.read`         | `{ path }`                                   | `data.read`               | `{ text }`, or `not_found` |
| `data.write`        | `{ path, text }`                             | `data.write`              | `{}` |
| `loopback.fetch`    | `{ port, path, method?, body?, timeoutMs? }` | `loopback:<port>`         | `{ status, text }` |
| `command.run`       | `{ name, args? }`                            | `command:<name>`          | `{ exitCode, stdout, stderr, timedOut, truncated }` |
| `preview.show`      | `{ path }`                                   | `preview.show`            | `{ opened: true, path }` |
| `notify`            | `{ title, body? }`                           | `notify`                  | `null` |
| `tab.params`        | —                                            | —                         | `{ params, caller }` |
| `tab.setTitle`      | `{ title }`                                  | —                         | `{}` |
| `tab.setBadge`      | `{ state, text? }`                           | —                         | `{}` |
| `ui.toast`          | `{ message, kind?, action? }`                | —                         | `{ action }` once it goes: whether its action was pressed |
| `ui.confirm`        | `{ message, confirm?, cancel?, danger? }`    | —                         | `true` or `false` |
| `ui.copy`           | `{ text }`                                   | —, from a click           | `{}` |

The three `ui.*` methods are new, and stand for what an HTML page had in `hangar.ui` and its own
document: a toast at the foot of the tab (`kind` among `info`, `success`, `warning`, `error`; it goes
after 4 seconds, an error stays until closed; 30 a minute), a question in place of the page (Cancel
first and holding the focus, Escape cancels; one at a time, a second answers `busy`), and the
clipboard. **`ui.copy` is refused (`denied`) unless the update that made it answers a person's click
or keystroke**: a tab may not fill the clipboard by itself, as a page may not.

How a call is held, in this order, by `hangar-kit-host` before the app sees it — the order of
`src/lib/kitApi.ts`, with its words:

1. At most **16 calls waiting** for their answer: `rate_limited`.
2. A method there is: `unknown`.
3. **What it needs, held now**: the tab's declared permissions that the person granted, read again at
   every call, so a grant taken back denies the next one. `loopback:<port>` for that very port,
   `command:<name>` for that very command. `denied`, with the message of an HTML tab.
4. **Params of the right shape and size**: paths relative to their folder, with either slash, no
   `..`, no drive or stream, no hidden part unless the tab holds both `files.read` and `files.write`
   (never `.git`), 1 024 characters; texts of 1 MB, images of 10 MB, `panes.send` of 20 000
   characters, a title of 40 and a badge of 12 on one plain line; `loopback.fetch`'s path, method,
   body and timeout as kits.md says. `invalid`, or `bad_args` for `command.run`'s values. A param
   left out and one sent as `null` are not the same, as they were not in kitApi.ts.
5. **What only the app can refuse before the rate** (`KitApi::check`): the workspace gone
   (`unavailable`), the pane `panes.send` names missing, not a Claude Code agent, or without the
   bridge. So a page that keeps naming the wrong pane spends none of the right one's sends.
6. **Rates**: `panes.send`, `data.write`, and `files.write` with `files.writeBinary`, 10 a minute
   each; `notify` 5; `tab.setTitle` and `tab.setBadge` 120 each; `ui.toast` 30. `rate_limited`.

The gate is public (`hangar_kit_host::Gate`): the app holds an HTML tab's calls, relayed from its
page by `hangar_webview::kit_relay`, with the same one, so the two kinds of tab are one set of rules.
An HTML tab is answered `unknown` for the three `ui.*` methods, which a page has in its own document.

Then the app does it, and answers what only it can: the pane, the file on disk where links can lead
out, the command's own rules, `not_found`, `conflict`, `unreachable`, `busy`, `failed`. A refused call
is answered like any other, later, as a `reply` with its `error`: a refusal is an ordinary state of a
tab, drawn, never a reason to stop it.

## The sandbox

| What                    | Limit                                                                                  |
| ----------------------- | -------------------------------------------------------------------------------------- |
| The system              | No WASI: no file, no clock but `hangar.now`, no environment, no randomness source, no thread, no network. All of it is a call, under a permission. |
| Imports                 | `hangar.log` and `hangar.now`. A module importing anything else does not load.          |
| Memory                  | 128 MB of linear memory per tab; one memory, 32-bit; tables of 100 000 entries.        |
| Time                    | `hangar_init` 1 s; `hangar_update` and `hangar_view` 250 ms each.                      |
| Module                  | 64 MB. Threads, shared memory, 64-bit and several memories are off.                    |
| Messages                | 4 MB each way.                                                                         |
| Calls                   | 16 waiting; the rates above.                                                            |
| Timers                  | 16, every 100 ms at the shortest.                                                       |
| Log                     | 200 lines of 4 KB kept per tab.                                                        |
| Instances               | One per open tab; two tabs of a kit share nothing but its data folder.                 |

**Time is bounded by epoch interruption.** A thread advances the engine's epoch every 10 ms, and
each call into a module gets a deadline in those ticks; past it the module traps where it stands.
Fuel would count instructions, the same on every machine, but costs on every instruction and
measures nothing a person waits on: what matters is that the window is never held for long, and the
deadline says it in milliseconds. A module never waits on Hangar (calls are tasks), so its time is
compute only, and 250 ms of it is far more than a page's update needs.

**Where it runs.** `KitTab` is called from the app's `update`, on the window's thread, which is why
the deadlines are short. It is `Send`: the app may move each tab to a worker thread later, the
drawing staying on the window's, without changing this contract.

**Limits are Hangar's, not the kit's** (`hangar_kit_host::Limits`): a kit cannot ask for more.

## When a tab fails

A tab that cannot start, or stops, **draws why in its place**: an alert, *This tab could not start*
or *This tab stopped*, the reason in words, a **Start it again** button, and the last lines it logged,
which are often the reason. Hangar never restarts a tab by itself, so a kit that fails at once does
not fail in a loop; the tab's badge is the app's to set.

| Kind        | When                                                                                      | Reason shown |
| ----------- | ----------------------------------------------------------------------------------------- | ------------ |
| `Load`      | Not WebAssembly; over 64 MB; imports what Hangar does not give (WASI, wasm-bindgen named as such); an export missing or of another type; another ABI. | *the tab imports wasi_snapshot_preview1::fd_write: it was built for WASI. Build it for wasm32-unknown-unknown…* |
| `Timeout`   | A call past its deadline.                                                                 | *it took longer than 250 ms answering an event: a tab must answer at once* |
| `Memory`    | Its memory grown past 128 MB (a Rust module traps when an allocation is refused).         | *it asked for 136 MB of memory while starting, more than the 128 MB a tab may use* |
| `Panic`     | The kit's code panicked: the SDK's panic hook logs the message at level `panic`, then the module traps. | *it panicked answering an event: panicked at src/lib.rs:88:9: …* |
| `Trap`      | Any other trap: out of bounds, an `unreachable` with no panic before it, a stack overflow. | the trap |
| `Message`   | An answer Hangar cannot read: not JSON, not a view, a buffer outside its memory, over 4 MB. | what was wrong, with serde's words |

When a tab stops, its instance is dropped with its memory, the calls it was waiting for are answered
to no one, and its timers stop. **Start it again** makes a new instance and runs `init` again. A
refused call, an unreachable server, a missing file are not failures: they are answers.

## For Hangar: opening a native tab

`hangar-kit-host` does not depend on hangar-core; the app is what knows both.

1. **One `KitRuntime`** for the app's life (`KitRuntime::new(Limits::default())`): the engine and its
   clock. **`compile`** a tab's module once per file — the slow part — keeping the `KitModule` until
   the file's modified time changes; every tab of it shares it.
2. **A `KitApi` per open tab**, from the app's own record of it (the kit, the workspace, the tab,
   never what the module says): `granted()` is the tab's declared permissions the workspace granted,
   read at each call, as `workspaceKitTabs` does today; `call(HostCall)` does the call with what
   hangar-core already has — `kits::files` (`files.*`, hidden files when `hidden`), `kit_scheme::files`
   (`kit_file_read`, `kit_workspace_path` for `files.read` without `files.write` and `preview.show`),
   `kits::data`, `kits::loopback`, `kits::commands` (with the commands' acceptance), the bridge's queue
   for `panes.send` (labelled `kit:<id>`, Claude Code panes with the bridge only, `not_found`,
   `unavailable`), the notifier (titled `<kit name> · <title>`), the tab strip for `tab.*`, and the
   window for `ui.*`. `image(source)` reads from the kit's folder or its data folder, under the same
   path rules.
3. **`KitTab::start`**, then: `draw(style)` in the view, mapped into the app's message with the tab's
   key; `update(message)` with what it sends; the `Effects` it answers run as tasks
   (`effects.task()`), each answering `KitMessage::Reply`; `subscription().with(key)` for its timers;
   `event("panes.changed", {})` to every tab of the workspace when the panes change; `set_visible`,
   `set_theme`, `set_params` when those change; dropping the `KitTab` when the tab closes or its
   workspace sleeps.
4. **`KitStyle::new(theme, locale)`** from the hangar-theme theme in use: Hangar UI's tokens, each
   the chrome token `tokens.json` maps it to (the values an HTML tab's theme event carries, which a
   test holds them to), the interface's faces, and the language of the words Hangar says itself (a tab
   that could not start, a widget it does not know). Buttons, icons, tooltips and menus are
   hangar-ui's. `KitStyle::dark()` and `light()` are graphite and github-light.

The app's side of all this is `crates/hangar-app/src/kit_tabs` (P16): one `KitApi` (`TabApi`) for
both kinds of tab, the page shown when a module does not load, the `ui.*` calls drawn at the foot of
the tab and in place of its page.

## Decisions taken

The person this was written for was not there to ask; these were decided, and can be revisited.

- **A native tab is an `entry` ending in `.wasm`, under schema 2**; a kit that must open on 1.x keeps
  an HTML `entry` and adds `native`, which an older Hangar ignores. The native Hangar is taken to be
  2.0.0.
- **The ABI is pull and push of whole messages, Elm-shaped**: `init`, `update`, `view`, subscriptions
  declared after every update, calls as tasks answered by a later update. No call blocks the module,
  so no import but `log` and `now` is needed, and a module's time is compute only.
- **JSON** for every message, for the reasons above.
- **Handlers are numbers in a view**, with the view's `seq`, the SDK keeping eight views' handlers,
  rather than string ids a kit must keep unique, or serialized messages a closure cannot be.
- **Epoch interruption, not fuel**, with deadlines of 1 s and 250 ms, and 128 MB of memory.
- **The rules of `src/lib/kitApi.ts` are in `hangar-kit-host`** (permissions, shapes, rates, the 16
  waiting), so every app embedding it holds a tab the same way; what needs the disk or the panes stays
  the app's.
- **Three `ui.*` methods** stand for `hangar.ui`'s toast, confirmation and clipboard, and **a
  `tab.visibility` event** for `document.hidden`. `ui.copy` needs a gesture, as a page's clipboard
  does.
- **`Start` carries the permissions held**, so a page can draw a denied state before it calls; a call
  is still the only authority.
- **Hangar keeps a table's order, a tree's open nodes and a text area's cursor**; inputs are
  controlled otherwise.
- **Images from the kit's folder need nothing; from its data folder, `data.read`.** Images from the
  workspace are not a widget source: a tab reads them with `files.readBinary`, which its permissions
  already cover. A source for bytes a kit holds is left for later.
- **`col!`, not `column!`**, for the SDK's macro: std has its own `column!()`, which a glob import of
  the prelude would make ambiguous.
- **The example and the test kit are workspace members**, built for the host too, with the host's
  tests building them for wasm32 into `target/kit-wasm` (needs the target installed, and nothing from
  the network).
- **Rows fill their width by default**, as a `.kit-bar` and a flex row do, so a `fill` space pushes
  what follows to the end.

## Later

- **The other controls**: text inputs, selects, checkboxes, switches, tables and trees are iced's
  own, styled from `KitStyle`, not yet hangar-ui's `input::*`. The contract does not change.
- **The failure reasons** (a module that does not load, a deadline passed) are the runtime's
  English: what a kit's author reads. The titles and the button around them are translated.
- **A long list** (`hangar.ui.list`, a hundred thousand lines) is not virtualised: a kit sends the
  lines in view of a `log` or `table` with a `height`. A windowed list widget, asking the kit for
  ranges, is the next widget to add.
- **Markdown**, through iced's `markdown`, a natural widget for kits that show agents' output.
- **Clock, date, since** in the SDK's `format`, once `Start` carries the person's time zone offset.
- **Accessibility**: iced has no accessibility tree yet; labels on icons and glyphs are tooltips until
  it does.
- **The progress bar's indeterminate state** is drawn still, the **split** is not draggable, and a
  tab cannot move the focus (which an HTML tab should not do by itself either).
- **Compiled modules on disk**: wasmtime's cache, so a tab opens without compiling after a restart.
