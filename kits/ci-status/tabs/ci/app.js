"use strict";
// Starts the tab in the page: wires the controller to the view.
(function () {
  const logic = window.CiLogic;
  const lang = logic.pickLang(navigator.language);
  document.documentElement.lang = lang;
  document.title = logic.t(lang, "tab");
  const container = document.getElementById("app");

  const controller = window.CiController.createController({
    hangar: window.hangar,
    logic,
    lang,
    onChange: draw,
  });
  const actions = {
    open: (id) => controller.openRun(id),
    back: () => controller.back(),
    retry: () => controller.retry(),
  };

  function draw(state) {
    window.CiView.render(container, state, { doc: document, logic, lang, now: Date.now, actions });
  }

  draw(controller.state);
  controller.start();
})();
