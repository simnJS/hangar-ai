"use strict";
// Starts the tab in the page: wires the controller to the view, in Hangar's language.
(function () {
  const logic = window.ChecklistLogic;
  const ui = window.hangar.ui;
  const t = ui
    ? ui.i18n(logic.STRINGS)
    : (key, vars) => logic.t(logic.pickLang(navigator.language), key, vars);
  const language = () => (ui ? ui.locale() : logic.pickLang(navigator.language));
  const container = document.getElementById("app");

  const controller = window.ChecklistController.createController({
    hangar: window.hangar,
    logic,
    t: (key, vars) => t(key, vars),
    onChange: draw,
  });

  const ctx = {
    doc: document,
    t,
    logic,
    actions: controller,
    summary: () => controller.summary(),
    copyButton: (button, source) => (ui ? ui.copyButton(button, source) : undefined),
  };

  function draw(state) {
    document.documentElement.lang = language();
    document.title = t("tab");
    window.ChecklistView.render(container, state, ctx);
  }

  // The first `theme` event may bring Hangar's language; a later one may change it.
  let lastLanguage = language();
  window.hangar.on("theme", () => {
    if (language() === lastLanguage) return;
    lastLanguage = language();
    draw(controller.state);
  });

  // Answers are written after a pause; leaving must not lose the last of them.
  window.addEventListener("pagehide", () => controller.flush());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") controller.flush();
  });

  draw(controller.state);
  controller.start();
})();
