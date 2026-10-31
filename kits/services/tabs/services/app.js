"use strict";
// Starts the tab in the page: wires the controller to the view.
(function () {
  const logic = window.ServicesLogic;
  const ui = window.hangar.ui;
  const t = ui.i18n(logic.STRINGS);
  const container = document.getElementById("app");

  const controller = window.ServicesController.createController({ hangar: window.hangar, logic, t, onChange: draw });
  const since = (at) => ui.format.since(Date.parse(at), Date.now());
  const actions = {
    refresh: () => controller.refresh(),
    filter: (id) => controller.setFilter(id),
    logs: (id) => controller.showLogs(id),
    ask: (key, target) => controller.openAsk(key, target),
    pick: (name) => controller.pickPane(name),
    send: () => controller.sendAsk(since),
    closeAsk: () => controller.closeAsk(),
  };

  function draw(state) {
    document.documentElement.lang = ui.locale();
    document.title = t("tab");
    window.ServicesView.render(container, state, { doc: document, logic, ui, t, now: Date.now, actions });
  }

  // Colours and sizes follow Hangar's theme by themselves, in kit.js and kit.css. Words do not.
  let locale = ui.locale();
  window.hangar.on("theme", () => {
    if (ui.locale() === locale) return;
    locale = ui.locale();
    draw(controller.state);
  });

  // Reads every 10 s while the tab is shown, and not while it is behind another.
  document.addEventListener("visibilitychange", () => controller.setVisible(document.visibilityState !== "hidden"));
  controller.setVisible(document.visibilityState !== "hidden");

  draw(controller.state);
  controller.start();
})();
