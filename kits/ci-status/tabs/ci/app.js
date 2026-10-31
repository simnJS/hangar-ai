"use strict";
// Starts the tab in the page: wires the controller to the view.
(function () {
  const logic = window.CiLogic;
  const ui = window.hangar.ui;
  const t = ui.i18n(logic.STRINGS);
  const container = document.getElementById("app");

  const controller = window.CiController.createController({
    hangar: window.hangar,
    logic,
    t,
    onChange: draw,
  });
  const actions = {
    open: (id) => controller.openRun(id),
    back: () => controller.back(),
    retry: () => controller.retry(),
  };

  function draw(state) {
    document.documentElement.lang = ui.locale();
    document.title = t("tab");
    window.CiView.render(container, state, { doc: document, logic, ui, t, now: Date.now, actions });
  }

  // Colours and sizes follow Hangar's theme by themselves, in kit.js and kit.css. Words do not.
  let locale = ui.locale();
  window.hangar.on("theme", () => {
    if (ui.locale() === locale) return;
    locale = ui.locale();
    controller.relabel();
    draw(controller.state);
  });

  // Ages keep moving on a page nothing else redraws: a finished run, the picker.
  setInterval(() => draw(controller.state), 60000);

  draw(controller.state);
  controller.start();
})();
