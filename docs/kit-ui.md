# Hangar UI

The interface library of kits' tabs, so that a tab looks like a part of Hangar rather than a page
framed inside it. A stylesheet of tokens and components, a script of behaviours, both served by
Hangar to every tab, with no dependency and nothing to install.

It is **tied to Hangar, not copied from it**. Its colours, measures, icons and font are generated from
Hangar's own sources, and a test fails when they no longer match: when Hangar changes a colour, a
radius or a size, the library follows, and so does every kit using it.

The contract of a kit, its manifest and its messages, is in [kits.md](kits.md).

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <link rel="stylesheet" href="/_hangar/kit.css" />
  </head>
  <body>
    <header class="kit-header">
      <div class="kit-header__main">
        <h1 class="kit-header__title">CI</h1>
        <p class="kit-header__state"><span class="kit-state kit-state--success">Passed</span> 2 h ago</p>
      </div>
      <div class="kit-header__actions">
        <button class="kit-btn kit-btn--small" type="button">Refresh</button>
      </div>
    </header>
    <main class="kit-page">…</main>
    <script src="/_hangar/kit.js"></script>
    <script src="/_hangar/kit-ui.js"></script>
  </body>
</html>
```

| Address               | What                                                                        |
| --------------------- | --------------------------------------------------------------------------- |
| `/_hangar/kit.css`    | Tokens and components. Enough on its own for a page without behaviours.     |
| `/_hangar/kit.js`     | The [SDK](kits.md#the-sdk), `window.hangar`. It keeps the page in Hangar's theme. |
| `/_hangar/kit-ui.js`  | Behaviours, `window.hangar.ui`. Load it after `kit.js`.                     |
| `/_hangar/inter.woff2` | The face `kit.css` asks for: you never name it.                            |

These addresses are absolute and are the only ones outside the kit a page's policy allows, each for
what it is: a stylesheet, scripts, a font. A page's own files stay relative.

## Versions

Hangar UI has no version of its own: it ships with Hangar. A kit says what it needs the usual way,
with `hangar` in its manifest:

```json
{ "hangar": ">=1.9.0" }
```

A kit that does not say so may open on an older Hangar, which answers these addresses with a 404:
its page then shows without a style.

| Component                                                                                           | Since Hangar |
| --------------------------------------------------------------------------------------------------- | ------------ |
| Layout: `.kit-page`, `.kit-fill`, `.kit-grow`, `.kit-stack`, `.kit-actions`, `.kit-bar`             | 1.9.0        |
| Text: `.kit-dim`, `.kit-muted`, `.kit-small`, `.kit-mono`, `.kit-num`, `.kit-truncate`, `.kit-wrap`, `.kit-sr`, `.kit-code` | 1.9.0 |
| Page header: `.kit-header`                                                                          | 1.9.0        |
| Section: `.kit-section`                                                                             | 1.9.0        |
| Row list: `.kit-rows`, `.kit-row`                                                                   | 1.9.0        |
| Button: `.kit-btn`                                                                                  | 1.9.0        |
| Icon: `.kit-icon`                                                                                   | 1.9.0        |
| Input: `.kit-input`, `.kit-field`                                                                   | 1.9.0        |
| Select: `.kit-select`                                                                               | 1.9.0        |
| Search field: `.kit-search`                                                                         | 1.9.0        |
| Switch: `.kit-switch`                                                                               | 1.9.0        |
| Chips: `.kit-chips`, `.kit-chip`                                                                    | 1.9.0        |
| Filter chip: `.kit-filter`                                                                          | 1.9.0        |
| Status dot: `.kit-dot`                                                                              | 1.9.0        |
| State glyph: `.kit-glyph`, `.kit-state`                                                             | 1.9.0        |
| Progress bar: `.kit-progress`                                                                       | 1.9.0        |
| Banner: `.kit-banner`                                                                               | 1.9.0        |
| Empty state: `.kit-empty`                                                                           | 1.9.0        |
| Code and log: `.kit-pre`, `.kit-log`, `.kit-logbox`, `.kit-list__chunk`                             | 1.9.0        |
| Key/value list: `.kit-kv`                                                                           | 1.9.0        |
| Tabs: `.kit-tabs`, `.kit-tab`, `.kit-tabpanel`                                                      | 1.9.0        |
| Inline confirmation: `.kit-confirm`                                                                 | 1.9.0        |
| Menu: `.kit-menu`                                                                                   | 1.9.0        |
| Toast: `.kit-toasts`, `.kit-toast`                                                                  | 1.9.0        |
| `hangar.ui.confirm`                                                                                 | 1.9.0        |
| `hangar.ui.toast`                                                                                   | 1.9.0        |
| `hangar.ui.menu`                                                                                    | 1.9.0        |
| `hangar.ui.tabs`                                                                                    | 1.9.0        |
| `hangar.ui.copy`, `hangar.ui.copyButton`                                                            | 1.9.0        |
| `hangar.ui.list`                                                                                    | 1.9.0        |
| `hangar.ui.log`, `hangar.ui.isErrorLine`                                                            | 1.9.0        |
| `hangar.ui.i18n`, `hangar.ui.locale`                                                                | 1.9.0        |
| `hangar.ui.format`                                                                                  | 1.9.0        |

A row is added here with the version that brings it.

## The rules a kit follows

- **State is a small glyph and a word, never a coloured page.** A failure takes `.kit-state--failure`
  in the header, not a red block across the tab. Colour in Hangar only ever means something, so
  most of the page has none.
- **One primary action per screen.** `.kit-btn--primary` is the one thing the page is for right
  now. Everything else is a default or a ghost button.
- **Empty, loading, error and permission-denied states are designed, never blank.** Each says what
  it is and what to do next: see [the empty state](#empty-state). A `denied` or `not_found` from
  the SDK is an ordinary state of a tab, not an exception to print.
- **Numbers are right-aligned and tabular**: `.kit-num`, or `.kit-row__end` which already is.
- **Long text wraps or truncates, with the full value on hover**: `.kit-truncate` and a `title`, or
  `.kit-wrap`. The page itself never scrolls sideways, down to the panel's narrowest, 320 px at 100 %
  interface size and 288 px at 90 %.
- **French and English strings of different lengths both fit.** Controls wrap rather than clip, and
  no width is set for one language. Try the longer one; `hangar.ui.i18n` holds both.
- **Everything is reachable by keyboard.** Use real `button`, `a`, `input` and `select` elements. A
  block that scrolls inside itself takes `tabindex="0"`. A `title` is a convenience for the
  pointer: what only it says, a keyboard never reads, so an icon alone also takes `aria-label`.
- **A page never takes the focus by itself.** Hangar hands the keyboard back to the terminal when
  a page grabs it unasked.
- **Write what the person can do, in the interface's voice.** "Install gh, then retry", not "An
  error occurred".

## Tokens

Every token is a custom property of the root element, named after what it means. Use them for
anything the components do not cover, and never write a colour of your own: a hex in a kit's sheet
is wrong on most of Hangar's themes.

| Colour                                                                  | What it is                                                   |
| ----------------------------------------------------------------------- | ------------------------------------------------------------ |
| `--kit-bg`                                                              | The page: the panel's own surface.                           |
| `--kit-surface`, `--kit-raised`, `--kit-float`                          | A group of rows; a button; what floats, a menu or a toast.   |
| `--kit-field`, `--kit-well`                                             | Inside an input; behind code and logs.                       |
| `--kit-hover`, `--kit-selected`                                         | A row under the pointer; the chosen row.                     |
| `--kit-seg-track`, `--kit-seg-thumb`                                    | Tabs: the recessed track and the chosen tab.                 |
| `--kit-text`, `--kit-text-dim`                                          | Text, on anything at 4.5:1. Dim is for labels and second lines. |
| `--kit-text-muted`, `--kit-text-faint`                                  | Metadata, placeholders: at 4.5:1 on plain surfaces only, not on `--kit-hover`, `--kit-selected` or a tint. |
| `--kit-line`, `--kit-border`, `--kit-border-hover`, `--kit-sep`         | A hairline between rows; a control's edge; that edge under the pointer; a line darker than everything around it. |
| `--kit-accent`, `--kit-on-accent`                                       | A primary fill; text on it.                                  |
| `--kit-accent-text`, `--kit-accent-soft`, `--kit-accent-ring`           | A link; a tint behind what is on; a quiet ring around it.    |
| `--kit-focus`                                                           | The focus ring, at 3:1 on every surface.                     |
| `--kit-ok`, `--kit-warn`, `--kit-danger`, `--kit-neutral`               | A state as a word, at 4.5:1.                                 |
| `--kit-ok-fill`, `--kit-warn-fill`, `--kit-neutral-fill`                | A state as a shape or a bar, at 3:1. `--kit-danger` is both. |
| `--kit-danger-soft`                                                     | A tint behind something destructive.                         |
| `--kit-shadow`, `--kit-float-ring`                                      | Under what floats.                                           |

| Measure                                                                 | What it is                                                     |
| ----------------------------------------------------------------------- | -------------------------------------------------------------- |
| `--kit-scale`                                                           | Hangar's interface size. Sizes below already follow it.        |
| `--kit-font`, `--kit-mono`                                              | Hangar's interface face, Inter, and the terminals' own.        |
| `--kit-fs-title`, `-body`, `-row`, `-control`, `-small`, `-code`, `-meta`, `-label`, `-chip` | The type scale: 15, 13, 12.5, 12, 11.5, 11.5, 11, 10.5, 10 px. |
| `--kit-h-row`, `-rich-row`, `-control`, `-small`, `-search`, `-filter`, `-head`, `-bar`, `-menu-item`, `-notice`, `-tabs` | Heights, from the rules of the same names in Hangar's stylesheets. |
| `--kit-radius-xs`, `-sm`, `--kit-radius`, `-lg`, `-xl`                  | 4, 5, 6, 8, 10 px.                                             |
| `--kit-space-2` … `--kit-space-24`                                      | 2, 4, 6, 8, 10, 12, 16, 20, 24 px: named after their size.     |
| `--kit-dur`, `--kit-ease`                                               | How long, and how, something responds.                         |
| `--kit-log-height`                                                      | How tall a code or log block grows before it scrolls. Set it on the block. |
| `--kit-icon-alert` and the others of [the icons](#icon)                 | An icon as a mask, for what a class does not cover.            |

Inter comes in its Latin part, which covers French and English. Other alphabets fall back to the system's
faces in the same stack.

## Components

Plain classes on plain HTML. [The gallery](#the-gallery) shows them all.

### Layout

```html
<body class="kit-fill">
  <div class="kit-bar">
    <label class="kit-search"><input type="search" placeholder="Search…" aria-label="Search" /></label>
    <button class="kit-btn kit-btn--small" type="button">Pause</button>
  </div>
  <div class="kit-grow">…the one part that scrolls…</div>
  <div class="kit-bar kit-bar--foot"><span class="kit-mono">editor.log</span><span class="kit-num">122 lines</span></div>
</body>
```

`.kit-page` is the padded column most pages are. `.kit-fill` is a page that does not scroll as a whole,
with `.kit-grow` the part that does. `.kit-stack` stacks, `.kit-actions` lines up controls and wraps them.

### Text

```html
<span class="kit-dim">Secondary</span> <span class="kit-muted">metadata</span> <span class="kit-small">a small aside</span>
<span class="kit-mono">a4f90c1</span> <span class="kit-num">1 204</span> <span class="kit-code">gh auth login</span>
<span class="kit-truncate" title="The whole of a long value">The whole of a long value</span>
<span class="kit-wrap">https://github.com/owner/name/actions/runs/41222364482</span>
<span class="kit-sr">Read aloud, not shown</span>
```

### Page header

A title, one line of state, actions at the right. Under `.kit-header__main`'s width the actions go below it.

```html
<header class="kit-header">
  <div class="kit-header__main">
    <h1 class="kit-header__title">Release 1.7.0</h1>
    <p class="kit-header__state"><span class="kit-state kit-state--failure">Failed</span> in 4 min 16 s</p>
  </div>
  <div class="kit-header__actions">
    <button class="kit-btn kit-btn--small kit-btn--primary" type="button">Run again</button>
  </div>
</header>
```

### Section

A title in small capitals, a count, and actions at the right.

```html
<section class="kit-section">
  <div class="kit-section__head">
    <h2 class="kit-section__title">Jobs</h2>
    <span class="kit-section__count">4</span>
    <div class="kit-section__actions"><button class="kit-btn kit-btn--small kit-btn--ghost" type="button">Expand</button></div>
  </div>
  <ul class="kit-rows">…</ul>
</section>
```

### Row list

A leading glyph, a title, a secondary line, a trailing value. Make the row a `button` or an `a` for it to
have a hover and take the keyboard. `.is-selected`, `aria-selected="true"` or `aria-current="true"` draw it
as the chosen one. Its title and second line truncate; `.kit-row--wrap` makes them wrap.

```html
<ul class="kit-rows">
  <li>
    <button class="kit-row is-selected" type="button">
      <span class="kit-row__lead"><span class="kit-glyph kit-glyph--failure" role="img" aria-label="Failed"></span></span>
      <span class="kit-row__main">
        <span class="kit-row__title" title="rust (macos-latest)">rust (macos-latest)</span>
        <span class="kit-row__sub">Failed at the step Tests</span>
      </span>
      <span class="kit-row__end">4 min 16 s</span>
    </button>
  </li>
  <li class="kit-row kit-row--wrap">
    <span class="kit-row__main"><span class="kit-row__sub">A long reason, shown whole over several lines.</span></span>
  </li>
</ul>
```

`.kit-rows--group` draws them as a box of Settings' rows, split by hairlines.

```html
<ul class="kit-rows kit-rows--group">
  <li class="kit-row">
    <span class="kit-row__main"><span class="kit-row__title">hangar-ai</span><span class="kit-row__sub">No kit enabled</span></span>
    <button class="kit-btn kit-btn--small" type="button">Manage</button>
  </li>
</ul>
```

### Button

```html
<button class="kit-btn" type="button">Default</button>
<button class="kit-btn kit-btn--primary" type="button">Primary</button>
<button class="kit-btn kit-btn--danger" type="button">Delete</button>
<button class="kit-btn kit-btn--ghost" type="button">Ghost</button>
<button class="kit-btn kit-btn--small" type="button">Small</button>
<button class="kit-btn kit-btn--icon" type="button" title="Run again" aria-label="Run again"><span class="kit-icon kit-icon--restart"></span></button>
<button class="kit-btn" type="button" disabled>Not now</button>
```

### Icon

Hangar's own, drawn in the colour of the text around them.

```html
<span class="kit-icon kit-icon--alert"></span>
<span class="kit-icon kit-icon--check"></span>
<span class="kit-icon kit-icon--chevron-down"></span>
<span class="kit-icon kit-icon--close"></span>
<span class="kit-icon kit-icon--copy"></span>
<span class="kit-icon kit-icon--info"></span>
<span class="kit-icon kit-icon--lock"></span>
<span class="kit-icon kit-icon--more"></span>
<span class="kit-icon kit-icon--plus"></span>
<span class="kit-icon kit-icon--restart"></span>
<span class="kit-icon kit-icon--search"></span>
```

### Input

`aria-invalid="true"` marks what was refused. `.kit-input` also goes on a `textarea`.

```html
<label class="kit-field">
  <span class="kit-field__label">Repository</span>
  <input class="kit-input" type="text" placeholder="owner/name" aria-invalid="true" />
  <span class="kit-field__hint">The workspace's own when empty.</span>
  <span class="kit-field__error">Write it as owner/name.</span>
</label>
```

### Select

```html
<span class="kit-select">
  <select aria-label="Branch">
    <option>main</option>
  </select>
</span>
```

### Search field

```html
<div class="kit-search">
  <input type="search" placeholder="Search a job…" aria-label="Search a job" />
  <button class="kit-search__clear" type="button" title="Clear" aria-label="Clear"></button>
</div>
```

### Switch

```html
<label class="kit-switch">
  <input type="checkbox" role="switch" checked />
  <span class="kit-switch__track"></span>
  Follow the end of the log
</label>
```

### Chips and filter chips

A chip is a short word something is filed under. A filter chip is a toggle: `aria-pressed` says it is on.

```html
<div class="kit-chips">
  <span class="kit-chip">rust</span>
  <button class="kit-filter" type="button" aria-pressed="true">Prints <span class="kit-filter__count">61</span></button>
  <button class="kit-filter" type="button" aria-pressed="false">Errors <span class="kit-filter__count kit-filter__count--danger">10</span></button>
</div>
```

### Status dot and state glyph

Shape and colour both say the state, so it reads for anyone who cannot tell green from amber. `running`
moves, subtly, unless the person asked for less motion. The words are the same as `tab.setBadge`'s, and the
glyphs are what the tab shows in Hangar's strip.

`.kit-state` is a glyph and a word, and how a page says a state. `.kit-glyph` is the glyph alone, as at the
head of a row. `.kit-dot` is the smallest, for what would crowd otherwise. Either alone takes `role="img"` and
an `aria-label`: a shape says nothing to a screen reader.

```html
<span class="kit-state kit-state--running">Running</span>
<span class="kit-state kit-state--success">Passed</span>
<span class="kit-state kit-state--failure">Failed</span>
<span class="kit-state kit-state--waiting">Queued</span>
<span class="kit-state kit-state--neutral">Cancelled</span>

<span class="kit-glyph kit-glyph--running" role="img" aria-label="Running"></span>
<span class="kit-glyph kit-glyph--success" role="img" aria-label="Passed"></span>
<span class="kit-glyph kit-glyph--failure" role="img" aria-label="Failed"></span>
<span class="kit-glyph kit-glyph--waiting" role="img" aria-label="Queued"></span>
<span class="kit-glyph kit-glyph--neutral" role="img" aria-label="Cancelled"></span>

<span class="kit-dot kit-dot--running" role="img" aria-label="Running"></span>
<span class="kit-dot kit-dot--success" role="img" aria-label="Passed"></span>
<span class="kit-dot kit-dot--failure" role="img" aria-label="Failed"></span>
<span class="kit-dot kit-dot--waiting" role="img" aria-label="Queued"></span>
<span class="kit-dot kit-dot--neutral" role="img" aria-label="Cancelled"></span>
```

### Progress bar

Determinate with a width on its fill. `--indeterminate` travels when there is nothing to measure against.
`--stale` is what it last knew, in dashes: nothing has been heard since, so it may no longer be true.

```html
<div class="kit-progress" role="progressbar" aria-label="Steps" aria-valuenow="40" aria-valuemin="0" aria-valuemax="100">
  <div class="kit-progress__fill" style="width: 40%"></div>
</div>
<div class="kit-progress kit-progress--indeterminate" role="progressbar" aria-label="Loading"><div class="kit-progress__fill"></div></div>
<div class="kit-progress kit-progress--stale" role="progressbar" aria-label="Last known"><div class="kit-progress__fill" style="width: 70%"></div></div>
<div class="kit-progress kit-progress--success" role="progressbar" aria-label="Steps"><div class="kit-progress__fill" style="width: 100%"></div></div>
<div class="kit-progress kit-progress--failure" role="progressbar" aria-label="Steps"><div class="kit-progress__fill" style="width: 62%"></div></div>
```

### Banner

A line in the flow of the page, for something that holds until it changes. Its text keeps the page's own
colour. What only just happened is [a toast](#toast).
An action never makes the line wider than the page: a long label wraps inside it, at 320 px and at 288 px.

```html
<p class="kit-banner"><span class="kit-banner__text">Refreshed every 30 seconds while the tab is visible.</span></p>
<p class="kit-banner kit-banner--warning"><span class="kit-banner__text">Showing 2,000 lines of 8,412.</span></p>
<p class="kit-banner kit-banner--error" role="alert">
  <span class="kit-banner__text">gh did not answer in 20 seconds. Check your connection, then try again.</span>
  <span class="kit-banner__action"><button class="kit-btn kit-btn--small" type="button">Try again</button></span>
</p>
```

### Empty state

The same element for all four things a part of a page can be instead of its content.

```html
<!-- Empty: what will fill it. -->
<div class="kit-empty">
  <p class="kit-empty__title">No run yet</p>
  <p class="kit-empty__text">A push to this repository starts one, and it shows here.</p>
</div>

<!-- Loading: what is being done. -->
<div class="kit-empty"><span class="kit-state kit-state--running">Reading the runs…</span></div>

<!-- Error: what went wrong, and what to do. -->
<div class="kit-empty">
  <span class="kit-icon kit-icon--alert kit-empty__icon"></span>
  <p class="kit-empty__title">gh is not installed</p>
  <p class="kit-empty__text">This tab reads the CI with GitHub CLI. Install it, sign in, then try again.</p>
  <div class="kit-empty__actions"><button class="kit-btn kit-btn--small kit-btn--primary" type="button">Try again</button></div>
</div>

<!-- Denied: what is missing, and where it is given. -->
<div class="kit-empty">
  <span class="kit-icon kit-icon--lock kit-empty__icon"></span>
  <p class="kit-empty__title">This tab may not read the CI</p>
  <p class="kit-empty__text">Accept the command <span class="kit-code">ci-run</span> in Settings › Kits.</p>
</div>
```

### Code and log

Mono, wrapped, scrolling inside itself. `--kit-log-height` sets how tall it grows. For a real log, let
[`hangar.ui.log`](#log) build it.

```html
<pre class="kit-pre" tabindex="0">gh run view 4122 --json status</pre>

<div class="kit-log" tabindex="0" style="--kit-log-height: 200px">
  <div class="kit-log__line kit-log__line--head"><span class="kit-log__text">rust › Tests</span></div>
  <div class="kit-log__line"><span class="kit-log__num">1</span><span class="kit-log__text">test a ... ok</span></div>
  <div class="kit-log__line kit-log__line--warning"><span class="kit-log__num">2</span><span class="kit-log__text">warning: unused</span></div>
  <div class="kit-log__line kit-log__line--error is-current"><span class="kit-log__num">3</span><span class="kit-log__text">error: test failed</span></div>
</div>
```

### Key/value list

```html
<dl class="kit-kv">
  <dt>Branch</dt>
  <dd>main</dd>
  <dt>Duration</dt>
  <dd>4 min 16 s</dd>
</dl>
```

### Tooltip

The browser's own, through `title`: no class is needed. Put the whole value on what is truncated, and the
name on what is only an icon. It answers the pointer, not the keyboard.

### Focus ring

Everything that takes the keyboard shows it, with nothing to add: `:focus-visible`, 2 px, at 3:1 on every
surface in every theme. Hangar's own ring is quieter, its accent at 45 %; kits' pages are held to the contrast.

## Behaviours

`window.hangar.ui`, from `/_hangar/kit-ui.js`, drawn with the classes above.

### Inline confirmation

`hangar.ui.confirm(button, { message, confirm, cancel, danger, signal })` asks where the button is, in place of it: a
sandboxed page has no `confirm()`. A Promise of `true` or `false`. Cancel comes first and holds the focus, so a
double click or a stray Enter never confirms; Escape cancels. `danger: false` makes the answer a primary button.
The page can dismiss it too, which answers `false`: pass `signal`, an `AbortSignal`, or call `cancel()` on the Promise.

```js
remove.addEventListener("click", async () => {
  if (await hangar.ui.confirm(remove, { message: "Delete this run?", confirm: "Delete" })) deleteRun();
});

// The run vanished meanwhile: take the question back.
const asked = hangar.ui.confirm(remove, { message: "Delete this run?" });
runGone.then(() => asked.cancel());
```

It draws this, in the classes `.kit-confirm` and `.kit-confirm__text`.

### Toast

`hangar.ui.toast(message, { kind, action, duration })`, at the foot of the page, for what just happened. `kind`
is `info`, `success`, `warning` or `error`. It goes after 4 seconds, while nobody points at it; an error stays
until it is closed. It answers `{ close() }`, and never takes the focus. Three at most show at once.

```js
hangar.ui.toast("Reservation released.", {
  kind: "success",
  action: { label: "Undo", onClick: reserveAgain },
});
```

Its parts are `.kit-toasts`, `.kit-toast`, `.kit-toast--success`, `.kit-toast--warning`, `.kit-toast--error` and
`.kit-toast__text`.

### Menu

`hangar.ui.menu(anchor, items, { align })` opens a dropdown under `anchor`, above it when there is no room, and
always on the page. Items are `{ label, onSelect, hint, danger, disabled, checked }`, and `"-"` for a line. Arrows,
Home, End, Enter and Escape work; a click outside closes it. Opening it again on the same anchor closes it,
so a button just calls it on each click. `align: "end"` lines up its right edges.

```js
more.addEventListener("click", () => {
  hangar.ui.menu(more, [
    { label: "Run again", onSelect: runAgain },
    { label: "Copy the link", hint: "Ctrl+C", onSelect: copyLink },
    "-",
    { label: "Delete", danger: true, onSelect: remove },
  ]);
});
```

Its parts are `.kit-menu`, `.kit-menu__item`, `.kit-menu__item--danger`, `.kit-menu__check`, `.kit-menu__label`,
`.kit-menu__hint` and `.kit-menu__sep`.

### Tabs

`hangar.ui.tabs(list, { onChange })` brings a tablist to life: clicks, arrows, Home and End, and one stop of the
Tab key for the whole list. More tabs than fit scroll inside the track. It answers `{ select(id) }`.

```html
<div class="kit-tabs" id="views" role="tablist" aria-label="View">
  <button class="kit-tab" id="tab-jobs" role="tab" type="button" aria-selected="true" aria-controls="panel-jobs">Jobs</button>
  <button class="kit-tab" id="tab-log" role="tab" type="button" aria-selected="false" aria-controls="panel-log">Log</button>
</div>
<div class="kit-tabpanel" id="panel-jobs" role="tabpanel" aria-labelledby="tab-jobs">…</div>
<div class="kit-tabpanel" id="panel-log" role="tabpanel" aria-labelledby="tab-log">…</div>
<script>
  hangar.ui.tabs(document.getElementById("views"));
</script>
```

### Copy

`hangar.ui.copyButton(button, source)` makes a button copy `source`, a string or a function giving one, and say
so for a moment. It swaps the `.kit-icon--copy` inside it for a check, or else its label. `hangar.ui.copy(text)`
is the bare thing: a Promise of whether it worked. It has to come from a click: a page may not fill the
clipboard by itself.

### Long list

`hangar.ui.list(scroller, { count, render, chunk, estimate })` keeps only what is near the view in the page, so a
hundred thousand lines cost what a few hundred do. Lines may wrap: it works by blocks (`.kit-list__chunk`), each
keeping its last measured height while out of the page. `render(index)` gives a line's element; `estimate` is a
line's height before it is measured, in pixels. A block keeps what was measured when `setCount` grows or
shrinks the list, so appending does not move what is being read further up.

```js
const rows = hangar.ui.list(scroller, {
  count: lines.length,
  render: (index) => {
    const row = document.createElement("div");
    row.textContent = lines[index];
    return row;
  },
});
rows.scrollTo(4500, { context: 3 });
rows.setCount(lines.length, { stick: true }); // After appending: a view at the end stays there.
rows.refresh(); // After the lines themselves changed.
rows.toEnd();
rows.element(4500); // Its element, when it is in the page right now.
rows.destroy();
```

The scroller needs a height of its own, and `overflow: auto`. A tab behind another has no size: what it was asked
to show waits until it has one.

### Log

`hangar.ui.log(root, { lines, label, isError, numbers, open })` builds a log in `root`, windowed by the list above,
that **opens on its first error**, with a few lines before it, or at its end when there is none. A line says
how many errors it holds, and with several offers to move between them. `lines` are strings or
`{ text, kind, time }`, `kind` being `error`, `warning` or `head`. `open: "start"` opens at the top instead.

```js
const log = hangar.ui.log(document.getElementById("log"), { lines, label: "Step log" });
log.append(moreLines); // A view at the end stays at the end.
log.setLines(otherLines);
log.next();
log.previous();
log.errors(); // Their indexes.
```

Without `isError`, a line is an error when `hangar.ui.isErrorLine(text)` says so: it reads the words of an error,
in English and in French, and not those of "0 errors" or `handles_failure_mode`. It is a guess, so a page that
knows its log's format should say. Its parts are `.kit-logbox`, `.kit-logbox__bar` and `.kit-logbox__count`.

### Language

`hangar.ui.i18n({ en: {…}, fr: {…} })` gives a `t(key, vars)` for the language Hangar is in, or the system's when
Hangar has not said, English filling in what a language lacks. `hangar.ui.locale()` says which it is now. A count
`n` picks `key_one` or `key_other` by the language's own rules, as in Hangar's dictionaries. Templates get
the same typography as Hangar's: no-break spaces between a number and its unit, and before French's high
punctuation. Only the template does: what fills it is shown as it was written.

```js
const t = hangar.ui.i18n({
  en: { jobs_one: "{n} job", jobs_other: "{n} jobs", failed: "Failed: {name}" },
  fr: { jobs_one: "{n} job", jobs_other: "{n} jobs", failed: "Échoué : {name}" },
});
t("jobs", { n: 2 }); // "2 jobs"
```

### Formats

`hangar.ui.format`, in the page's language, and written the way Hangar writes them.

| Call                                  | French                     | English                  |
| ------------------------------------- | -------------------------- | ------------------------ |
| `format.duration(ms)`                 | `2,3 s`, `1 min 28 s`, `2 h 05 min`, `3 j 4 h` | `2.3 s`, `1 min 28 s`, `2 h 05 min`, `3 d 4 h` |
| `format.clock(at, { seconds: true })` | `21:38:12`                 | `21:38:12`               |
| `format.date(at, now)`                | `31 oct., 21:38`           | `Oct 31, 09:38 PM`       |
| `format.since(at, now)`               | `il y a 5 min`, `hier à 21 h 13` | `5 min ago`, `yesterday at 9:13 PM` |
| `format.bytes(size)`                  | `15 Ko`                    | `15 KB`                  |
| `format.number(value, options)`       | `1 204,5`                  | `1,204.5`                |

`at` is a `Date` or milliseconds. `now`, in the two that take it, is for a page that keeps its own clock.

## The theme

Until Hangar speaks, `prefers-color-scheme` decides between the sheet's two sets of values, Hangar's default
dark theme and its default light one. So a page opened outside Hangar looks right too.

### What the host sends

The frame's page cannot read Hangar's theme, so the window tells it, by an event of the
[tab message protocol](kits.md#the-tab-message-protocol). `kit.js` applies it by itself, before any handler the
page registered with `hangar.on("theme", …)`.

```json
{
  "hangar": 1,
  "event": "theme",
  "data": {
    "scheme": "dark",
    "locale": "fr",
    "tokens": {
      "--kit-bg": "#1b1c1f",
      "--kit-text": "#c8c9ce",
      "--kit-scale": "1",
      "--kit-mono": "\"Cascadia Mono\", monospace"
    }
  }
}
```

- **To whom**: the frame's `contentWindow`, target origin `"*"`, as every event: its origin is opaque.
- **When**: at each `load` of the frame, so a reload is themed again, and whenever the active theme, the language,
  the interface size or the terminals' font changes. No permission is asked: it says nothing about the workspace.
- **`scheme`**: `"dark"` or `"light"`, the theme's `appearance`.
- **`locale`**: Hangar's language, `"en"` or `"fr"`, once the setting is resolved. It reaches `hangar.ui.locale`.
- **`tokens`**: all of these, as strings:
  - one entry per line of `colors` in `crates/hangar-core/kit-sdk/tokens.json`, the key being the token and the
    value the theme's chrome token named on that line (`hangar_theme::kit_tokens`). The map is included from
    that file rather than copied, so a token added there reaches the host by itself;
  - `--kit-scale`, the interface size, `"1"` at 100 %;
  - `--kit-mono`, the value the app gives `--mono`: the terminals' font.

```rust
// crates/hangar-app/src/kit_tabs/tabs.rs
pub fn kit_theme_event(theme: &Theme, ui_scale: f64, mono: &str, locale: Locale) -> Value {
    let mut tokens = Map::new();
    tokens.insert("--kit-scale".into(), json!(ui_scale.to_string()));
    tokens.insert("--kit-mono".into(), json!(mono));
    for (token, value) in hangar_theme::kit_tokens(theme) {
        tokens.insert(token, json!(value));
    }
    hangar_webview::kit_relay::event(
        "theme",
        json!({ "scheme": theme.appearance.as_str(), "locale": locale.id(), "tokens": tokens }),
    )
}
```

`hangar-theme`'s `kit` tests hold what this sends, on every bundled theme, to the contrast below.

### What the page does with it

- A token name must be `--kit-…`; anything else is ignored. So is a message not from the frame's parent.
- A token the next event leaves out goes back to the sheet's value.
- The root element gets `data-kit-scheme`, `color-scheme` and `data-kit-locale`. `kit.css` needs nothing else.
- Between its first paint and the event, a page follows `prefers-color-scheme`. A person whose system and
  Hangar disagree can see that for an instant, until the frame's `load`.

A page may force a scheme with `<html data-kit-scheme="dark">`, as the gallery does.

## Contrast

By the window's own rule (`hangar_theme::contrast`, `TEXT_CONTRAST` and `FILL_CONTRAST`): text at 4.5:1, a state's
shape and the focus ring at 3:1. `hangar-theme`'s `kit::tests::contrast` checks every pair of the components, on
all of Hangar's themes and not only the two the sheet writes, tints included: an error line, a banner, a filter
left on. A component that writes a new pair adds it there.

## How it is made

| Part                                    | From                                                                                 |
| --------------------------------------- | ------------------------------------------------------------------------------------ |
| Colours, in both schemes                | `hangar-theme`'s `graphite` and `github-light`, through `crates/hangar-core/kit-sdk/tokens.json` (`cargo run -p hangar-theme --example kit-tokens`). |
| Measures, type scale, font stacks       | Written in `scripts/gen-hangar-ui.mjs`: the last values of the web app's stylesheets, which left with it (tag `v1.12.1`). A size moved in `hangar_theme::tokens` is moved there too. |
| Icons                                   | The drawings of `hangar-ui`'s icon table (`crates/hangar-ui/src/icon/data.rs`).     |
| The font and its `@font-face`           | `@fontsource-variable/inter`, the package Hangar Mobile loads it from (`pnpm install` first). |
| The components, and the `--kit-space-*` | By hand. Hangar has no spacing scale of its own: these are the steps its sheets use. |

The generated part sits between two marked comments at the top of `kit.css`. After a change in Hangar that the test
reports (a theme of the two, an icon, the font package):

```sh
node scripts/gen-hangar-ui.mjs           # writes kit.css and the font beside it
node scripts/gen-hangar-ui.mjs --check   # fails when either is stale
```

If a token or an icon the library follows goes away, the script stops and names it, rather than keep the old value.

## The gallery

`crates/hangar-core/kit-sdk/gallery.html` shows every component and state in both themes, at 320 px and wide, for reviewing
by eye. Open the file in a browser. It is not served to kits. `?only=dark-320` shows one of the four, and
`?locale=en` changes what the library itself says.
