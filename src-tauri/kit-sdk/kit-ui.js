/*
 * Hangar UI's behaviours, served at /_hangar/kit-ui.js, loaded after
 * /_hangar/kit.js (docs/kit-ui.md). No dependency.
 *
 * What a page should not have to write again, and what its sandbox takes away
 * from it: no confirm() and no alert(), a clipboard API it may not use. It fills
 * window.hangar.ui, drawn with the classes of /_hangar/kit.css.
 */
(function () {
  "use strict";
  if (!window.hangar) {
    console.error("Hangar UI: load /_hangar/kit.js before /_hangar/kit-ui.js");
    return;
  }
  if (window.hangar.ui) return;

  /* ---------- Language ---------- */

  /** What Hangar said (the `theme` event), or else what the system says. */
  function locale() {
    const tag = document.documentElement.getAttribute("data-kit-locale") || navigator.language || "en";
    return tag.split("-")[0].toLowerCase();
  }

  /* The same rules as src/lib/typography.ts, and a test holds them to it. */
  const NBSP = " ";
  const NARROW_NBSP = " ";
  const SPACE = "[ \\u00a0\\u202f]";
  const UNIT = "(?:min|ms|s|h|j|d|sem)";
  const BEFORE_UNIT = new RegExp(`(\\d|\\})${SPACE}(${UNIT})(?![\\p{L}\\p{N}])`, "gu");
  const AFTER_UNIT = new RegExp(`(${NBSP}${UNIT})${SPACE}(?=\\d|\\{)`, "gu");
  const AFTER_SIGN = new RegExp(`([<>≤≥~])${SPACE}(?=\\d|\\{)`, "gu");
  const BEFORE_HIGH = new RegExp(`${SPACE}+([?!;:»%])`, "g");
  const AFTER_OPENING = new RegExp(`«${SPACE}*`, "g");

  function typeset(text, lang) {
    const bound = text
      .replace(BEFORE_UNIT, `$1${NBSP}$2`)
      .replace(AFTER_UNIT, `$1${NBSP}`)
      .replace(AFTER_SIGN, `$1${NBSP}`);
    return lang === "fr"
      ? bound.replace(BEFORE_HIGH, `${NARROW_NBSP}$1`).replace(AFTER_OPENING, `«${NARROW_NBSP}`)
      : bound;
  }

  /**
   * `t(key, vars)` over `{ en: {...}, fr: {...} }`, English filling in what a
   * language lacks. A count `n` picks `key_one` or `key_other`, by the language's
   * own rules. Only the template is typeset: what fills it is shown as written.
   */
  function i18n(dictionaries) {
    return function t(key, vars) {
      const lang = locale();
      const values = vars || {};
      const plural = typeof values.n === "number" ? new Intl.PluralRules(lang).select(values.n) : null;
      const find = (dictionary) => {
        if (!dictionary) return undefined;
        if (plural) {
          const form = dictionary[`${key}_${plural}`] ?? dictionary[`${key}_other`];
          if (form !== undefined) return form;
        }
        return dictionary[key];
      };
      const template = find(dictionaries[lang]) ?? find(dictionaries.en);
      if (template === undefined) return key;
      return typeset(template, lang).replace(/\{(\w+)\}/g, (whole, name) =>
        name in values ? String(values[name]) : whole,
      );
    };
  }

  const say = i18n({
    en: {
      cancel: "Cancel",
      confirm: "Confirm",
      close: "Close",
      copy: "Copy",
      copied: "Copied",
      copyFailed: "Could not copy",
      errors_one: "{n} error",
      errors_other: "{n} errors",
      errorAt: "Error {at} of {n}",
      previous: "Previous",
      next: "Next",
      justNow: "just now",
      minutesAgo: "{n} min ago",
      todayAt: "today at {time}",
      yesterdayAt: "yesterday at {time}",
      dateAt: "{date} at {time}",
    },
    fr: {
      cancel: "Annuler",
      confirm: "Confirmer",
      close: "Fermer",
      copy: "Copier",
      copied: "Copié",
      copyFailed: "Copie impossible",
      errors_one: "{n} erreur",
      errors_other: "{n} erreurs",
      errorAt: "Erreur {at} sur {n}",
      previous: "Précédente",
      next: "Suivante",
      justNow: "à l'instant",
      minutesAgo: "il y a {n} min",
      todayAt: "aujourd'hui à {time}",
      yesterdayAt: "hier à {time}",
      dateAt: "le {date} à {time}",
    },
  });

  /* ---------- Formats ---------- */

  const number = (value, options) => new Intl.NumberFormat(locale(), options).format(value);

  /** "900 ms", "2,3 s", "1 min 28 s", "2 h 05 min", "3 j 4 h": what Hangar writes. */
  function duration(ms) {
    if (typeof ms !== "number" || !Number.isFinite(ms) || ms < 0) return "";
    const lang = locale();
    if (ms < 1000) return typeset(`${Math.round(ms)} ms`, lang);
    if (ms < 10000) return typeset(`${number(ms / 1000, { maximumFractionDigits: 1 })} s`, lang);
    const total = Math.round(ms / 1000);
    if (total < 60) return typeset(`${total} s`, lang);
    const seconds = total % 60;
    const minutes = Math.floor(total / 60) % 60;
    const hours = Math.floor(total / 3600) % 24;
    const days = Math.floor(total / 86400);
    const two = (value) => String(value).padStart(2, "0");
    if (days > 0) return typeset(`${days} ${lang === "fr" ? "j" : "d"}${hours ? ` ${hours} h` : ""}`, lang);
    if (hours > 0) return typeset(`${hours} h${minutes ? ` ${two(minutes)} min` : ""}`, lang);
    return typeset(`${minutes} min${seconds ? ` ${seconds} s` : ""}`, lang);
  }

  const toDate = (at) => (at instanceof Date ? at : new Date(at));
  const startOfDay = (at) => {
    const day = new Date(at);
    day.setHours(0, 0, 0, 0);
    return day.getTime();
  };

  /** A time in a column, where times have to line up: "21:14", or "21:14:56". */
  function clock(at, options) {
    return toDate(at).toLocaleTimeString(locale(), {
      hour: "2-digit",
      minute: "2-digit",
      ...(options && options.seconds ? { second: "2-digit" } : {}),
      hourCycle: "h23",
    });
  }

  /** A moment, on its own: "31 oct., 21:14", with the year when it is not this one. */
  function date(at, now) {
    const when = toDate(at);
    const sameYear = when.getFullYear() === new Date(now ?? Date.now()).getFullYear();
    return new Intl.DateTimeFormat(locale(), {
      day: "numeric",
      month: "short",
      ...(sameYear ? {} : { year: "numeric" }),
      hour: "2-digit",
      minute: "2-digit",
    }).format(when);
  }

  /** A time inside a sentence: "20 h 01" in French, "8:01 PM" in English. */
  function proseTime(at, lang) {
    const when = toDate(at);
    if (lang === "fr") return `${when.getHours()}${NBSP}h${NBSP}${String(when.getMinutes()).padStart(2, "0")}`;
    return when.toLocaleTimeString(lang, { hour: "numeric", minute: "2-digit" });
  }

  /** How long ago, as the inspector says it: "il y a 5 min", "hier à 21 h 13". */
  function since(at, now) {
    const lang = locale();
    const when = toDate(at).getTime();
    const current = toDate(now ?? Date.now()).getTime();
    const minutes = Math.floor((current - when) / 60000);
    if (minutes < 1) return say("justNow");
    if (minutes < 60) return say("minutesAgo", { n: minutes });
    const time = proseTime(when, lang);
    const days = Math.round((startOfDay(current) - startOfDay(when)) / 86400000);
    if (days === 0) return say("todayAt", { time });
    if (days === 1) return say("yesterdayAt", { time });
    const day = new Date(when).toLocaleDateString(lang, { day: "numeric", month: "short" });
    return say("dateAt", { date: day, time });
  }

  /** "15 Ko", "15 KB": the French write octets. */
  function bytes(size) {
    const units = locale() === "fr" ? ["o", "Ko", "Mo", "Go", "To"] : ["B", "KB", "MB", "GB", "TB"];
    let value = size;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    const digits = value < 10 && unit > 0 ? 1 : 0;
    return `${number(value, { maximumFractionDigits: digits })}${NBSP}${units[unit]}`;
  }

  /* ---------- A few elements ---------- */

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function actionButton(label, className, onClick) {
    const node = el("button", `kit-btn kit-btn--small ${className || ""}`.trim(), label);
    node.type = "button";
    node.addEventListener("click", onClick);
    return node;
  }

  /* ---------- Inline confirmation ---------- */

  const asking = new WeakMap();

  /**
   * Asks where the button is, in place of it, since a sandboxed page has no
   * confirm(). A Promise of true or false. Cancel comes first and has the focus,
   * so a double click or a stray Enter never confirms.
   */
  function confirm(trigger, options) {
    if (asking.has(trigger)) return asking.get(trigger);
    const settings = options || {};
    const answer = new Promise((resolve) => {
      const box = el("span", "kit-confirm");
      box.setAttribute("role", "group");
      if (settings.message) box.append(el("span", "kit-confirm__text", settings.message));
      const finish = (result) => {
        box.remove();
        trigger.hidden = false;
        asking.delete(trigger);
        trigger.focus();
        resolve(result);
      };
      const cancel = actionButton(settings.cancel || say("cancel"), "", () => finish(false));
      const yes = actionButton(
        settings.confirm || say("confirm"),
        settings.danger === false ? "kit-btn--primary" : "kit-btn--danger",
        () => finish(true),
      );
      box.append(cancel, yes);
      box.addEventListener("keydown", (event) => {
        if (event.key === "Escape") finish(false);
      });
      trigger.hidden = true;
      trigger.after(box);
      cancel.focus();
    });
    asking.set(trigger, answer);
    return answer;
  }

  /* ---------- Toast ---------- */

  const MAX_TOASTS = 3;
  let toasts = null;

  /**
   * A short message at the foot of the page, for what just happened: `kind` is
   * "info", "success", "warning" or "error". An error stays until it is closed:
   * nobody should have to be looking at the right moment. Never takes the focus.
   */
  function toast(message, options) {
    const settings = options || {};
    const kind = settings.kind || "info";
    if (!toasts || !toasts.isConnected) {
      toasts = el("div", "kit-toasts");
      toasts.setAttribute("aria-live", "polite");
      document.body.append(toasts);
    }
    const node = el("div", kind === "info" ? "kit-toast" : `kit-toast kit-toast--${kind}`);
    node.setAttribute("role", kind === "error" ? "alert" : "status");
    node.append(el("span", "kit-toast__text", message));
    let timer = 0;
    const close = () => {
      clearTimeout(timer);
      node.remove();
    };
    if (settings.action) {
      node.append(
        actionButton(settings.action.label, "kit-btn--ghost", () => {
          close();
          settings.action.onClick();
        }),
      );
    }
    const dismiss = el("button", "kit-btn kit-btn--ghost kit-btn--small kit-btn--icon");
    dismiss.type = "button";
    dismiss.title = say("close");
    dismiss.setAttribute("aria-label", say("close"));
    dismiss.append(el("span", "kit-icon kit-icon--close"));
    dismiss.addEventListener("click", close);
    node.append(dismiss);
    while (toasts.children.length >= MAX_TOASTS) toasts.firstChild.remove();
    toasts.append(node);
    const lasts = settings.duration ?? (kind === "error" ? 0 : 4000);
    const start = () => {
      if (lasts > 0) timer = setTimeout(close, lasts);
    };
    node.addEventListener("pointerenter", () => clearTimeout(timer));
    node.addEventListener("pointerleave", start);
    node.addEventListener("focusin", () => clearTimeout(timer));
    node.addEventListener("focusout", start);
    start();
    return { close };
  }

  /* ---------- Menu ---------- */

  let openMenu = null;

  /**
   * A dropdown under `anchor`. `items` are `{ label, onSelect, hint?, danger?,
   * disabled?, checked? }` and "-" for a line between them. Opening it again on
   * the same anchor closes it, so a button can just call this on each click.
   */
  function menu(anchor, items, options) {
    const reopening = openMenu && openMenu.anchor === anchor;
    if (openMenu) openMenu.close(false);
    if (reopening) return { close() {} };
    const settings = options || {};
    const panel = el("div", "kit-menu");
    panel.setAttribute("role", "menu");
    panel.tabIndex = -1;
    const checkable = items.some((item) => item !== "-" && item.checked !== undefined);

    function close(refocus) {
      if (!panel.isConnected) return;
      panel.remove();
      anchor.setAttribute("aria-expanded", "false");
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", dropped);
      window.removeEventListener("blur", dropped);
      document.removeEventListener("scroll", scrolled, true);
      openMenu = null;
      if (refocus !== false) anchor.focus();
    }
    const dropped = () => close(false);
    const scrolled = (event) => {
      if (!panel.contains(event.target)) close(false);
    };
    function outside(event) {
      if (panel.contains(event.target) || anchor.contains(event.target)) return;
      close(false);
    }

    for (const item of items) {
      if (item === "-") {
        const line = el("div", "kit-menu__sep");
        line.setAttribute("role", "separator");
        panel.append(line);
        continue;
      }
      const row = el("button", item.danger ? "kit-menu__item kit-menu__item--danger" : "kit-menu__item");
      row.type = "button";
      row.tabIndex = -1;
      row.disabled = Boolean(item.disabled);
      row.setAttribute("role", checkable ? "menuitemcheckbox" : "menuitem");
      if (checkable) {
        row.setAttribute("aria-checked", String(Boolean(item.checked)));
        const check = el("span", "kit-menu__check");
        if (item.checked) check.append(el("span", "kit-icon kit-icon--check"));
        row.append(check);
      }
      const label = el("span", "kit-menu__label", item.label);
      label.title = item.label;
      row.append(label);
      if (item.hint) row.append(el("span", "kit-menu__hint", item.hint));
      row.addEventListener("click", () => {
        close();
        if (item.onSelect) item.onSelect();
      });
      panel.append(row);
    }

    const live = () => Array.from(panel.querySelectorAll(".kit-menu__item:not(:disabled)"));
    panel.addEventListener("keydown", (event) => {
      const rows = live();
      const at = rows.indexOf(document.activeElement);
      const go = (index) => rows[(index + rows.length) % rows.length]?.focus();
      if (event.key === "ArrowDown") go(at + 1);
      else if (event.key === "ArrowUp") go(at < 0 ? -1 : at - 1);
      else if (event.key === "Home") go(0);
      else if (event.key === "End") go(-1);
      else if (event.key === "Escape") close();
      // Back on the anchor, so the key moves on from there rather than from nowhere.
      else if (event.key === "Tab") return close();
      else return;
      event.preventDefault();
      event.stopPropagation();
    });

    document.body.append(panel);
    // Under the anchor when it fits, above it otherwise, and always on the page.
    const gap = 4;
    const margin = 8;
    const from = anchor.getBoundingClientRect();
    const size = panel.getBoundingClientRect();
    const width = document.documentElement.clientWidth;
    const height = window.innerHeight;
    const wanted = settings.align === "end" ? from.right - size.width : from.left;
    panel.style.left = `${Math.max(margin, Math.min(wanted, width - size.width - margin))}px`;
    const below = from.bottom + gap;
    const above = from.top - gap - size.height;
    const top = below + size.height <= height - margin || above < margin ? below : above;
    panel.style.top = `${Math.max(margin, Math.min(top, height - size.height - margin))}px`;

    anchor.setAttribute("aria-haspopup", "menu");
    anchor.setAttribute("aria-expanded", "true");
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", dropped);
    window.addEventListener("blur", dropped);
    document.addEventListener("scroll", scrolled, true);
    openMenu = { anchor, close };
    (live()[0] || panel).focus({ preventScroll: true });
    return { close: () => close() };
  }

  /* ---------- Tabs ---------- */

  /**
   * Brings a `role="tablist"` to life: each `role="tab"` shows the panel its
   * `aria-controls` names, and arrows, Home and End move between them, the list
   * being one stop of the Tab key. Answers `{ select(id) }`, by the tab's id.
   */
  function tabs(list, options) {
    const settings = options || {};
    const all = () => Array.from(list.querySelectorAll('[role="tab"]'));
    function select(tab, focus) {
      for (const other of all()) {
        const chosen = other === tab;
        other.setAttribute("aria-selected", String(chosen));
        other.tabIndex = chosen ? 0 : -1;
        const panel = document.getElementById(other.getAttribute("aria-controls"));
        if (panel) panel.hidden = !chosen;
      }
      // Inside the track only: choosing a tab never moves the page.
      const shown = tab.getBoundingClientRect();
      const track = list.getBoundingClientRect();
      if (shown.left < track.left) list.scrollLeft -= track.left - shown.left;
      else if (shown.right > track.right) list.scrollLeft += shown.right - track.right;
      if (focus) tab.focus({ preventScroll: true });
    }
    list.addEventListener("click", (event) => {
      const tab = event.target.closest('[role="tab"]');
      if (!tab || !list.contains(tab) || tab.getAttribute("aria-selected") === "true") return;
      select(tab);
      if (settings.onChange) settings.onChange(tab.id);
    });
    list.addEventListener("keydown", (event) => {
      const rows = all();
      const at = rows.indexOf(document.activeElement);
      if (at < 0) return;
      const moves = { ArrowRight: at + 1, ArrowLeft: at - 1, Home: 0, End: rows.length - 1 };
      if (!(event.key in moves)) return;
      event.preventDefault();
      const tab = rows[(moves[event.key] + rows.length) % rows.length];
      select(tab, true);
      if (settings.onChange) settings.onChange(tab.id);
    });
    const current = all().find((tab) => tab.getAttribute("aria-selected") === "true") || all()[0];
    if (current) select(current);
    return {
      select(id) {
        const tab = all().find((candidate) => candidate.id === id);
        if (tab) select(tab);
      },
    };
  }

  /* ---------- Copy ---------- */

  /**
   * Puts text on the clipboard: a Promise of whether it did. A sandboxed frame
   * has no use of the clipboard API as a rule, so the old way goes first; it
   * needs the click it was asked from.
   */
  async function copy(text) {
    const focused = document.activeElement;
    const area = el("textarea", "kit-sr");
    area.value = text;
    area.setAttribute("aria-hidden", "true");
    area.tabIndex = -1;
    area.style.position = "fixed";
    document.body.append(area);
    area.focus({ preventScroll: true });
    area.select();
    let copied = false;
    try {
      copied = document.execCommand("copy");
    } catch (_) {
      copied = false;
    }
    area.remove();
    if (focused && typeof focused.focus === "function") focused.focus({ preventScroll: true });
    if (copied) return true;
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (_) {
      return false;
    }
  }

  /** A button that copies `source`, a string or a function giving one, and says so. */
  function copyButton(button, source) {
    const icon = button.querySelector(".kit-icon--copy");
    const words = icon ? null : button.textContent;
    const label = button.getAttribute("aria-label");
    const hint = button.title;
    let timer = 0;
    button.addEventListener("click", async () => {
      const copied = await copy(typeof source === "function" ? source() : source);
      const word = copied ? say("copied") : say("copyFailed");
      clearTimeout(timer);
      if (icon) {
        icon.classList.toggle("kit-icon--copy", !copied);
        icon.classList.toggle("kit-icon--check", copied);
        button.title = word;
        button.setAttribute("aria-label", word);
      } else {
        button.textContent = word;
      }
      timer = setTimeout(() => {
        if (icon) {
          icon.classList.add("kit-icon--copy");
          icon.classList.remove("kit-icon--check");
          button.title = hint;
          if (label === null) button.removeAttribute("aria-label");
          else button.setAttribute("aria-label", label);
        } else {
          button.textContent = words;
        }
      }, 1500);
    });
  }

  /* ---------- A long list, windowed ---------- */

  /**
   * Only what is near the view is in the page, so a hundred thousand lines cost
   * what a few hundred do. Lines may wrap: a list works by blocks of lines, each
   * keeping the height it was last measured at while it is out of the page.
   * `scroller` is the element that scrolls; `render(index)` gives a line's element.
   */
  function list(scroller, options) {
    const render = options.render;
    const size = options.chunk || 200;
    const estimate = options.estimate || 17;
    let count = 0;
    let waiting = null;
    const chunks = [];
    const byElement = new WeakMap();

    const linesIn = (chunk) => Math.min(size, count - chunk.first);
    const atEnd = () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 4;

    function fill(chunk) {
      const before = chunk.el.offsetHeight;
      const above = chunk.el.getBoundingClientRect().bottom <= scroller.getBoundingClientRect().top;
      const lines = document.createDocumentFragment();
      const total = linesIn(chunk);
      for (let index = 0; index < total; index++) lines.append(render(chunk.first + index));
      chunk.el.replaceChildren(lines);
      chunk.el.style.height = "";
      chunk.filled = total;
      // A block that grows above the view must not push what is being read.
      if (above) scroller.scrollTop += chunk.el.offsetHeight - before;
    }

    function toEnd() {
      if (!scroller.clientHeight) {
        waiting = { end: true };
        return;
      }
      // The last block's real height, or the end moves once it is drawn.
      const last = chunks[chunks.length - 1];
      if (last && !last.filled) fill(last);
      scroller.scrollTop = scroller.scrollHeight;
    }

    function hollow(chunk) {
      chunk.el.style.height = `${chunk.el.offsetHeight}px`;
      chunk.el.replaceChildren();
      chunk.filled = 0;
    }

    const watcher = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const chunk = byElement.get(entry.target);
          if (!chunk) continue;
          if (entry.isIntersecting && !chunk.filled) fill(chunk);
          else if (!entry.isIntersecting && chunk.filled) hollow(chunk);
        }
      },
      { root: scroller, rootMargin: "100% 0px" },
    );

    function setCount(next, settings) {
      const stick = Boolean(settings && settings.stick) && atEnd();
      count = next;
      while (chunks.length && chunks[chunks.length - 1].first >= count) {
        const last = chunks.pop();
        watcher.unobserve(last.el);
        last.el.remove();
      }
      while (chunks.length * size < count) {
        const chunk = { first: chunks.length * size, el: el("div", "kit-list__chunk"), filled: 0 };
        byElement.set(chunk.el, chunk);
        chunks.push(chunk);
        scroller.append(chunk.el);
        watcher.observe(chunk.el);
      }
      for (const chunk of chunks) {
        if (chunk.filled) {
          if (chunk.filled !== linesIn(chunk)) fill(chunk);
        } else {
          chunk.el.style.height = `${linesIn(chunk) * estimate}px`;
        }
      }
      if (stick) toEnd();
    }

    function scrollTo(index, settings) {
      if (!scroller.clientHeight) {
        waiting = { index, settings };
        return;
      }
      const at = Math.floor(index / size);
      const chunk = chunks[at];
      if (!chunk) return;
      // The real height of what is around it, or the line lands somewhere else.
      for (const near of [chunks[at - 1], chunk, chunks[at + 1]]) {
        if (near && !near.filled) fill(near);
      }
      const line = chunk.el.children[index - chunk.first];
      if (!line) return;
      const offset = line.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
      const context = (settings && settings.context) || 0;
      scroller.scrollTop += offset - context * estimate;
    }

    // A tab behind another has no size: what it was asked to show waits for one.
    const sized = new ResizeObserver(() => {
      if (waiting && scroller.clientHeight) {
        const { end, index, settings } = waiting;
        waiting = null;
        if (end) toEnd();
        else scrollTo(index, settings);
      }
    });
    sized.observe(scroller);

    setCount(options.count || 0);
    return {
      setCount,
      scrollTo,
      /** Draws again what is in the page, after the lines themselves changed. */
      refresh() {
        for (const chunk of chunks) if (chunk.filled) fill(chunk);
      },
      /** The element of a line, when it is in the page right now. */
      element(index) {
        const chunk = chunks[Math.floor(index / size)];
        return chunk && chunk.filled ? chunk.el.children[index - chunk.first] || null : null;
      },
      toEnd,
      destroy() {
        watcher.disconnect();
        sized.disconnect();
        scroller.replaceChildren();
      },
    };
  }

  /* ---------- Log ---------- */

  // A word alone: not part of a name like `handles_failure_mode` or `errors-kit`.
  const ERROR_WORDS =
    /(?:^|[^-_/\\.\p{L}\p{N}])(?:errors?|failed|failures?|fatal|panic|panicked|exception|erreurs?|échecs?|échouée?s?)(?![-_\p{L}\p{N}])(?!\s*[:=]\s*0\b)/iu;
  const NOT_ERROR =
    /(?:^|[^\p{L}\p{N}])(?:0|no|zero|without|sans|aucune?)\s+(?:errors?|failures?|failed|erreurs?|échecs?)(?![\p{L}\p{N}])/iu;

  /** Whether a log line reads as an error, unless the page knows better. */
  const isErrorLine = (text) => ERROR_WORDS.test(text) && !NOT_ERROR.test(text);

  /**
   * A log that opens where the trouble is: on its first error, with a few lines
   * before it, or at its end when there is none. `lines` are strings or `{ text,
   * kind?, time? }`, `kind` being "error", "warning" or "head".
   */
  function log(root, options) {
    const settings = options || {};
    const classify = settings.isError || isErrorLine;
    let lines = [];
    let errors = [];
    let current = -1;

    const kindOf = (line) => line.kind || (classify(line.text) ? "error" : "");
    const normal = (line) => (typeof line === "string" ? { text: line } : line);

    root.classList.add("kit-logbox");
    root.replaceChildren();
    const bar = el("div", "kit-logbox__bar");
    const count = el("span", "kit-logbox__count");
    const previous = actionButton(say("previous"), "kit-btn--ghost", () => go(current - 1));
    const next = actionButton(say("next"), "kit-btn--ghost", () => go(current + 1));
    bar.append(count, previous, next);
    const scroller = el("div", "kit-log");
    scroller.tabIndex = 0;
    if (settings.label) scroller.setAttribute("aria-label", settings.label);
    root.append(bar, scroller);

    function renderLine(index) {
      const line = lines[index];
      const kind = kindOf(line);
      const row = el("div", kind ? `kit-log__line kit-log__line--${kind}` : "kit-log__line");
      if (errors[current] === index) row.classList.add("is-current");
      if (settings.numbers !== false) row.append(el("span", "kit-log__num", String(index + 1)));
      if (line.time) row.append(el("span", "kit-log__num", line.time));
      row.append(el("span", "kit-log__text", line.text));
      return row;
    }

    const rows = list(scroller, { render: renderLine, estimate: settings.estimate || 17 });

    function sayCount() {
      bar.hidden = errors.length === 0;
      previous.hidden = next.hidden = errors.length < 2;
      count.textContent =
        current >= 0 && errors.length > 1
          ? say("errorAt", { at: current + 1, n: errors.length })
          : say("errors", { n: errors.length });
    }

    function mark(index, on) {
      const row = rows.element(index);
      if (row) row.classList.toggle("is-current", on);
    }

    function go(position) {
      if (!errors.length) return;
      if (current >= 0) mark(errors[current], false);
      current = (position + errors.length) % errors.length;
      rows.scrollTo(errors[current], { context: 3 });
      mark(errors[current], true);
      sayCount();
    }

    function findErrors(from) {
      for (let at = from; at < lines.length; at++) if (kindOf(lines[at]) === "error") errors.push(at);
    }

    function setLines(given, opening) {
      lines = given.map(normal);
      errors = [];
      current = -1;
      findErrors(0);
      rows.setCount(0);
      rows.setCount(lines.length);
      sayCount();
      const place = opening || settings.open || "error";
      if (place === "error" && errors.length) go(0);
      else if (place !== "start") rows.toEnd();
    }

    /** More lines at the end: a view at the end stays there, and nothing else moves. */
    function append(more) {
      const start = lines.length;
      for (const line of more) lines.push(normal(line));
      findErrors(start);
      rows.setCount(lines.length, { stick: true });
      sayCount();
    }

    setLines(settings.lines || []);
    return {
      setLines,
      append,
      errors: () => errors.slice(),
      next: () => go(current + 1),
      previous: () => go(current - 1),
      scrollTo: rows.scrollTo,
    };
  }

  window.hangar.ui = Object.freeze({
    confirm,
    toast,
    menu,
    tabs,
    copy,
    copyButton,
    list,
    log,
    isErrorLine,
    i18n,
    locale,
    format: Object.freeze({ number, duration, clock, date, since, bytes }),
  });
})();
