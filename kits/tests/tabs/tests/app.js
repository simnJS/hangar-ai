"use strict";
// Starts the tab in the page: wires the controller to the view, and Hangar UI's language
// and formats to both.
(function () {
  const logic = window.TestsLogic;
  const { ui } = window.hangar;
  const currentLang = () => logic.pickLang(ui ? ui.locale() : navigator.language);
  const t = ui ? ui.i18n(logic.STRINGS) : null;
  const container = document.getElementById("app");
  let lang = currentLang();
  let tick = null;

  const actions = {
    handOver: (key, index) => controller.handOver(key, index),
    choose: (key, agent) => controller.choose(key, agent),
    retry: () => controller.retry(),
  };

  function draw(state) {
    tick = window.TestsView.render(container, state, {
      doc: document,
      logic,
      lang,
      now: Date.now,
      actions,
      t,
      format: ui ? ui.format : null,
    });
  }

  function applyLang() {
    lang = currentLang();
    document.documentElement.lang = lang;
    document.title = logic.t(lang, "tab");
  }

  const controller = window.TestsController.createController({
    hangar: window.hangar,
    logic,
    lang,
    onChange: draw,
    onTick: () => tick && tick(),
  });

  // Hangar says its language with its theme, once the frame has loaded and at each change.
  window.hangar.on("theme", () => {
    const before = lang;
    applyLang();
    if (lang === before) return;
    controller.setLang(lang);
    draw(controller.state);
  });

  applyLang();
  draw(controller.state);
  controller.start();
})();
