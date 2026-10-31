"use strict";
// Starts the tab in the page: wires the controller to the view.
(function () {
  const logic = window.GuardLogic;
  const lang = logic.pickLang(navigator.language);
  document.documentElement.lang = lang;
  document.title = logic.t(lang, "tab");
  const container = document.getElementById("app");

  const controller = window.GuardController.createController({
    hangar: window.hangar,
    logic,
    lang,
    onChange: draw,
  });
  const actions = {
    allow: (key) => controller.allow(key),
    clear: () => controller.clear(),
    retry: () => controller.retry(),
  };

  function draw(state) {
    window.GuardView.render(container, state, { doc: document, logic, lang, now: Date.now, actions });
  }

  draw(controller.state);
  controller.start();
})();
