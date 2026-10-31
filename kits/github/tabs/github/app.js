"use strict";
// Starts the tab in the page: wires the controller to the view, Hangar UI's
// language, formats, menu and toasts to both, and the 60 s refresh to the tab's visibility.
(function () {
  const logic = window.GithubLogic;
  const kind = document.body.dataset.kind === "issues" ? "issues" : "pulls";
  const { ui } = window.hangar;
  const currentLang = () => logic.pickLang(ui ? ui.locale() : navigator.language);
  let lang = currentLang();
  const t = ui ? ui.i18n(logic.STRINGS) : null;
  const say = (key, vars) => (t ? t(key, vars) : logic.t(lang, key, vars));
  const container = document.getElementById("app");

  const toast = (message, toastKind) => {
    if (ui && ui.toast) ui.toast(message, { kind: toastKind });
  };

  async function hand(anchor, number) {
    const found = await controller.panes();
    if (!found.ok) {
      toast(say(found.kind === "list-denied" ? "listDenied" : "noAgent"), "warning");
      return;
    }
    const label = { working: "agentWorking", waiting: "agentWaiting", yours: "agentYours" };
    const items = found.panes.map((pane) => ({
      label: pane.name,
      hint: label[pane.state] ? say(label[pane.state]) : undefined,
      onSelect: async () => {
        const result = await controller.handOff(number, pane.name);
        if (result.ok) toast(say("sent", { pane: result.pane }), "success");
        else if (result.kind === "read") toast(say("briefFailed"), "error");
        else if (result.kind === "send-denied") toast(say("sendDenied"), "error");
        else if (result.kind !== "busy") toast(say("sendFailed", { detail: result.detail || "" }), "error");
      },
    }));
    if (ui && ui.menu) ui.menu(anchor, items);
  }

  const actions = {
    refresh: () => controller.refresh(),
    open: (number) => controller.open(number),
    back: () => controller.back(),
    filters: (patch) => controller.setFilters(patch),
    clearFilters: () => controller.clearFilters(),
    hand,
    copy: (button, text) => ui && ui.copy && ui.copy(text).then((ok) => ok && toast(say("copied"), "success")),
  };

  function draw(state) {
    window.GithubView.render(container, state, {
      doc: document,
      logic,
      lang,
      kind,
      now: Date.now,
      actions,
      t,
      format: ui ? ui.format : null,
    });
  }

  function applyLang() {
    lang = currentLang();
    document.documentElement.lang = lang;
    document.title = logic.t(lang, kind === "issues" ? "tabIssues" : "tabPulls");
  }

  const controller = window.GithubController.createController({ hangar: window.hangar, logic, kind, lang, onChange: draw });

  // Hangar says its language with its theme, once the frame has loaded and at each change.
  window.hangar.on("theme", () => {
    const before = lang;
    applyLang();
    if (lang === before) return;
    controller.setLang(lang);
    draw(controller.state);
  });
  document.addEventListener("visibilitychange", () => controller.setVisible(document.visibilityState !== "hidden"));

  applyLang();
  draw(controller.state);
  controller.setVisible(document.visibilityState !== "hidden");
  controller.start();
})();
