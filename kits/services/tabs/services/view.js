"use strict";
// Draws the controller's state with Hangar UI's classes (docs/kit-ui.md), with elements
// and text nodes made by hand: nothing a program, a container or a log said is parsed as
// markup. The controller calls this every ten seconds and at every click, so each part
// is rebuilt only when what it shows changed: a redraw must not take the focus from a
// button, close a question, or scroll a log back.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.ServicesView = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  /** Fills `host` from `build`, unless what it shows is still `key`. */
  function part(host, key, build) {
    if (host.svcKey === key) return;
    host.svcKey = key;
    host.replaceChildren(...build());
  }

  function render(container, state, ctx) {
    const { doc, logic, ui, t, now, actions } = ctx;

    function el(tag, className, text) {
      const element = doc.createElement(tag);
      if (className) element.className = className;
      if (text !== undefined && text !== null) element.textContent = text;
      return element;
    }
    const button = (label, className, handler) => {
      const element = el("button", `kit-btn kit-btn--small ${className}`.trim(), label);
      element.type = "button";
      element.onclick = handler;
      return element;
    };
    const truncated = (tag, className, text) => {
      const element = el(tag, className, text);
      element.title = text;
      return element;
    };
    const glyph = (kind, said) => {
      const element = el("span", `kit-glyph kit-glyph--${kind}`);
      element.setAttribute("role", "img");
      element.setAttribute("aria-label", said);
      return element;
    };
    const since = (at) => ui.format.since(Date.parse(at), now());
    const range = (base) => ({ from: base, to: base + logic.BLOCK_SIZE - 1 });

    function empty(title, hint, { icon, actionsNode } = {}) {
      const box = el("div", "kit-empty");
      if (icon) box.append(el("span", `kit-icon kit-icon--${icon} kit-empty__icon`));
      box.append(el("p", "kit-empty__title", title), el("p", "kit-empty__text", hint));
      if (actionsNode) box.append(actionsNode);
      return box;
    }

    function tryAgain() {
      const wrap = el("div", "kit-empty__actions");
      wrap.append(button(t("tryAgain"), "kit-btn--primary", actions.refresh));
      return wrap;
    }

    // ---- the skeleton, made once -------------------------------------------------------
    if (!container.svcSkeleton) {
      const page = el("main", "kit-page svc-page");
      const header = el("header", "kit-header");
      const main = el("div", "kit-header__main");
      const title = el("h1", "kit-header__title");
      const stateLine = el("p", "kit-header__state");
      main.append(title, stateLine);
      const headActions = el("div", "kit-header__actions");
      const refresh = button("", "kit-btn--primary", () => actions.refresh());
      headActions.append(refresh);
      header.append(main, headActions);
      const portsSection = el("section", "kit-section");
      const portsHead = el("div", "kit-section__head");
      const portsBody = el("div", "svc-body");
      portsSection.append(portsHead, portsBody);
      const dockerSection = el("section", "kit-section");
      const dockerHead = el("div", "kit-section__head");
      const dockerBody = el("div", "svc-body");
      dockerSection.append(dockerHead, dockerBody);
      const foot = el("p", "kit-small kit-muted svc-foot");
      page.append(header, portsSection, dockerSection, foot);
      container.replaceChildren(page);
      container.svcSkeleton = { title, stateLine, refresh, portsHead, portsBody, dockerSection, dockerHead, dockerBody, foot };
    }
    const skeleton = container.svcSkeleton;
    const minute = Math.floor(now() / 60000);
    const locale = ui.locale();

    skeleton.title.textContent = t("tab");
    skeleton.refresh.textContent = state.refreshing ? t("refreshing") : t("refresh");
    skeleton.refresh.disabled = state.refreshing;
    skeleton.foot.textContent = t("auto");
    skeleton.stateLine.textContent = state.updatedAt ? t("updated", { time: ui.format.clock(state.updatedAt, { seconds: true }) }) : "";

    if (!state.loaded) {
      part(skeleton.portsHead, `loading|${locale}`, () => [el("h2", "kit-section__title", t("ports"))]);
      part(skeleton.portsBody, `loading|${locale}`, () => {
        const box = el("div", "kit-empty");
        box.append(el("span", "kit-state kit-state--running", t("loading")));
        return [box];
      });
      part(skeleton.dockerHead, `loading|${locale}`, () => [el("h2", "kit-section__title", t("containers"))]);
      part(skeleton.dockerBody, `loading|${locale}`, () => []);
      return;
    }

    // Nothing can be read at all: one state says it, not two.
    const bothDenied = state.ports.status === "denied" && state.docker.status === "denied";
    skeleton.dockerSection.hidden = bothDenied;

    // ---- ports ---------------------------------------------------------------------------
    const rows = logic.portRows(state.ports.items, state.panes, state.docker.items);
    const inBlocks = rows.filter((row) => row.owner);
    const shown = state.filter === "blocks" ? inBlocks : rows;

    part(skeleton.portsHead, `${locale}|${state.ports.status}|${rows.length}|${inBlocks.length}|${state.filter}|${bothDenied}`, () => {
      if (bothDenied) return [];
      const nodes = [el("h2", "kit-section__title", t("ports"))];
      if (state.ports.status === "ok") {
        nodes.push(el("span", "kit-section__count", String(rows.length)));
        if (inBlocks.length) {
          const chips = el("div", "kit-section__actions kit-chips");
          for (const [id, label, count] of [["all", t("allPorts"), rows.length], ["blocks", t("onlyBlocks"), inBlocks.length]]) {
            const chip = el("button", "kit-filter");
            chip.type = "button";
            chip.setAttribute("aria-pressed", String(state.filter === id));
            chip.append(label, " ", el("span", "kit-filter__count", String(count)));
            chip.onclick = () => actions.filter(id);
            chips.append(chip);
          }
          nodes.push(chips);
        }
      }
      return nodes;
    });

    const askKeyPort = (row) => `port:${row.port}:${row.pid}`;
    const portsKey = JSON.stringify([bothDenied, locale, minute, state.ports, state.docker.items.map((c) => [c.id, c.ports.map((p) => p.host)]), state.panes && state.panes.map((p) => [p.name, p.canSend]), state.panesDenied, state.filter, state.ask && [state.ask.key, state.ask.pane, state.ask.status, state.ask.detail]]);
    part(skeleton.portsBody, portsKey, () => {
      if (state.ports.status === "denied") return [deniedBox()];
      if (state.ports.status === "error") return [portsFault()];
      if (!rows.length) return [empty(t("noPorts"), t("noPortsHint"))];
      const list = el("ul", "kit-rows");
      for (const row of shown) {
        const item = el("li", "svc-item");
        item.append(portRow(row));
        if (state.ask && state.ask.key === askKeyPort(row)) item.append(askPanel());
        list.append(item);
      }
      const nodes = [list];
      if (state.ports.notes.includes("some-hidden")) nodes.push(el("p", "kit-small kit-muted svc-note", t("hiddenNote")));
      return nodes;
    });

    function ownerLine(row) {
      const owner = row.owner;
      if (!owner) return row.container ? t("dockerPort", { name: row.container.name }) : null;
      if (owner.kind === "worker") return t(owner.gone ? "blockGone" : "blockOf", { name: owner.name });
      return t("blockFree", range(owner.base));
    }

    function portRow(row) {
      const line = el("div", "kit-row kit-row--wrap svc-row");
      const main = el("span", "kit-row__main");
      const heading = el("span", "kit-row__title svc-heading");
      heading.append(el("span", "kit-num svc-port", String(row.port)), " ", truncated("span", "svc-program", row.name || t("unknownProgram")));
      heading.title = `${row.port} ${row.name || ""}`.trim();
      const facts = [];
      if (row.pid !== null) facts.push(t("pid", { pid: row.pid }));
      if (row.startedAt) facts.push(t("started", { since: since(row.startedAt) }));
      facts.push(t(row.loopback ? "loopback" : "exposed"));
      main.append(heading, el("span", "kit-row__sub kit-wrap", facts.join(" · ")));
      const owner = ownerLine(row);
      if (owner) {
        const ownerNode = el("span", `kit-row__sub kit-wrap${row.owner && row.owner.kind === "worker" ? " svc-owner" : ""}`, owner);
        main.append(ownerNode);
      }
      const buttons = el("span", "kit-actions svc-actions");
      buttons.append(askButton(askKeyPort(row), { type: "port", row }, t("portWhat", { port: row.port })));
      main.append(buttons);
      line.append(main);
      return line;
    }

    function askButton(key, target, what) {
      const open = state.ask && state.ask.key === key;
      const element = button(t("ask"), "kit-btn--ghost", () => actions.ask(key, target));
      element.setAttribute("aria-expanded", String(Boolean(open)));
      element.title = t("askTitle", { what });
      return element;
    }

    function askPanel() {
      const ask = state.ask;
      const box = el("div", "svc-ask");
      const what = ask.target.type === "container" ? t("containerWhat", { name: ask.target.container.name }) : t("portWhat", { port: ask.target.row.port });
      box.append(el("p", "kit-field__label svc-ask-title", t("askTitle", { what })));
      if (state.panesDenied) {
        box.append(banner("error", t("askDenied")), closeRow());
        return box;
      }
      const sendable = (state.panes || []).filter((pane) => pane.canSend);
      if (state.panes === null) {
        box.append(banner("error", t("askNoPanes")), closeRow());
        return box;
      }
      if (!sendable.length) {
        box.append(banner("warning", t("askNone")), closeRow());
        return box;
      }
      const field = el("label", "kit-field");
      field.append(el("span", "kit-field__label", t("askTo")));
      const wrap = el("span", "kit-select");
      const select = el("select");
      select.setAttribute("aria-label", t("askTo"));
      const blank = el("option", null, "—");
      blank.value = "";
      select.append(blank);
      for (const pane of sendable) {
        const option = el("option", null, pane.name);
        option.value = pane.name;
        if (pane.name === ask.pane) option.selected = true;
        select.append(option);
      }
      select.value = ask.pane;
      select.onchange = () => actions.pick(select.value);
      wrap.append(select);
      field.append(wrap);
      box.append(field);
      const brief = el("pre", "kit-pre svc-brief", logic.briefFor(ask.target, t, since));
      brief.tabIndex = 0;
      box.append(brief);
      if (ask.status === "sent") box.append(banner("info", t("askSent", { pane: ask.pane })));
      if (ask.status === "error") box.append(banner("error", ask.detail === "denied" ? t("askDenied") : t("askFailed", { detail: ask.detail })));
      const actionsRow = el("div", "kit-actions");
      const send = button(t("askSend"), "kit-btn--primary", () => actions.send());
      send.disabled = ask.status === "sending" || ask.status === "sent";
      actionsRow.append(send, button(t("askCancel"), "kit-btn--ghost", () => actions.closeAsk()));
      box.append(actionsRow);
      return box;
    }

    function closeRow() {
      const wrap = el("div", "kit-actions");
      wrap.append(button(t("askCancel"), "kit-btn--ghost", () => actions.closeAsk()));
      return wrap;
    }

    function banner(kind, text) {
      const p = el("p", `kit-banner${kind === "info" ? "" : ` kit-banner--${kind}`}`);
      if (kind === "error") p.setAttribute("role", "alert");
      p.append(el("span", "kit-banner__text", text));
      return p;
    }

    function deniedBox() {
      return empty(t("deniedTitle"), t("deniedHint"), { icon: "lock" });
    }

    function portsFault() {
      const kind = state.ports.kind;
      const [title, hint] = kind === "no-node" ? ["errNoNodeTitle", "errNoNodeHint"] : kind === "no-tool" ? ["errNoToolTitle", "errNoToolHint"] : kind === "timeout" ? ["errTimeoutTitle", "errTimeoutHint"] : ["errPortsTitle", "errPortsHint"];
      return empty(t(title), t(hint), { icon: "alert", actionsNode: tryAgain() });
    }

    // ---- containers ----------------------------------------------------------------------
    part(skeleton.dockerHead, `${locale}|${state.docker.status}|${state.docker.items.length}|${bothDenied}`, () => {
      if (bothDenied) return [];
      const nodes = [el("h2", "kit-section__title", t("containers"))];
      if (state.docker.status === "ok") nodes.push(el("span", "kit-section__count", String(state.docker.items.length)));
      return nodes;
    });

    const dockerKey = JSON.stringify([bothDenied, locale, state.docker, state.panes && state.panes.map((p) => [p.name, p.canSend]), state.panesDenied, state.logs, state.ask && [state.ask.key, state.ask.pane, state.ask.status, state.ask.detail]]);
    part(skeleton.dockerBody, dockerKey, () => {
      const docker = state.docker;
      if (bothDenied) return [];
      if (docker.status === "denied") return [deniedBox()];
      if (docker.status === "absent") return [empty(t("dockerAbsentTitle"), t("dockerAbsentHint"), { icon: "info" })];
      if (docker.status === "stopped") return [empty(t("dockerStoppedTitle"), t("dockerStoppedHint"), { icon: "info", actionsNode: tryAgain() })];
      if (docker.status === "error") {
        const box = empty(t(docker.kind === "timeout" ? "errTimeoutTitle" : "dockerFailedTitle"), docker.kind === "timeout" ? t("errTimeoutHint") : docker.detail || t("errPortsHint"), { icon: "alert", actionsNode: tryAgain() });
        return [box];
      }
      if (!docker.items.length) return [empty(t("noContainers"), t("noContainersHint"))];
      const list = el("ul", "kit-rows");
      for (const item of docker.items) {
        const li = el("li", "svc-item");
        li.append(containerRow(item));
        if (state.logs && state.logs.id === item.id) li.append(logsBlock(item));
        if (state.ask && state.ask.key === `container:${item.id}`) li.append(askPanel());
        list.append(li);
      }
      return [list];
    });

    function containerRow(item) {
      const status = logic.containerState(item);
      const stateWord = t(status.key, status.vars);
      const line = el("div", "kit-row kit-row--wrap svc-row");
      const lead = el("span", "kit-row__lead");
      lead.append(glyph(status.glyph, stateWord));
      const main = el("span", "kit-row__main");
      main.append(truncated("span", "kit-row__title", item.name), el("span", "kit-row__sub kit-wrap", [stateWord, item.image].filter(Boolean).join(" · ")));
      main.append(el("span", "kit-row__sub kit-wrap kit-mono", item.portsText || t("noPublished")));
      const blocks = [...new Set(item.ports.map((p) => logic.blockOf(p.host)).filter((base) => base !== null))];
      if (blocks.length) main.append(el("span", "kit-row__sub kit-wrap", blocks.map((base) => t("blockFree", range(base))).join(" · ")));
      const buttons = el("span", "kit-actions svc-actions");
      const logsOpen = state.logs && state.logs.id === item.id;
      const logsButton = button(logsOpen ? t("hideLogs") : t("logs"), "kit-btn--ghost", () => actions.logs(item.id));
      logsButton.setAttribute("aria-expanded", String(Boolean(logsOpen)));
      buttons.append(logsButton, askButton(`container:${item.id}`, { type: "container", container: item }, t("containerWhat", { name: item.name })));
      main.append(buttons);
      line.append(lead, main);
      return line;
    }

    function logsBlock(item) {
      const logs = state.logs;
      const box = el("div", "svc-logs");
      if (logs.status === "loading") {
        const loading = el("div", "kit-empty");
        loading.append(el("span", "kit-state kit-state--running", t("logsLoading")));
        box.append(loading);
      } else if (logs.status === "error") {
        box.append(banner("error", t("logsFailed", { detail: logs.detail || "?" })));
      } else if (!logs.lines.length) {
        box.append(el("p", "kit-small kit-muted svc-note", t("logsEmpty")));
      } else {
        const host = el("div", "svc-logbox");
        ui.log(host, { lines: logs.lines, label: t("logsLabel", { name: item.name }) });
        box.append(host, el("p", "kit-small kit-muted svc-note", t("logsTail", { n: logic.LOG_LINES })));
      }
      return box;
    }
  }

  return { render };
});
