"use strict";
// Starts the tab in the page: wires the controller to the view, and Hangar UI's language and
// formats to both.
(function () {
  const logic = window.DbLogic;
  const { ui } = window.hangar;
  const currentLang = () => logic.pickLang(ui ? ui.locale() : navigator.language);
  const t = ui ? ui.i18n(logic.STRINGS) : null;
  const container = document.getElementById("app");
  let lang = currentLang();

  const actions = {
    refresh: () => controller.refresh(),
    pick: (path) => controller.pick(path),
    selectTable: (name) => controller.selectTable(name),
    setPanel: (id) => controller.setPanel(id),
    sortBy: (index) => controller.sortBy(index),
    goTo: (offset) => controller.goTo(offset),
    toggleFull: (row, column) => controller.toggleFull(row, column),
    choose: (agent) => controller.choose(agent),
    openHandOver: () => controller.openHandOver(),
    closeHandOver: () => controller.closeHandOver(),
    setQuestion: (text) => controller.setQuestion(text),
    sendHandOver: () => controller.sendHandOver(),
  };

  function draw(state) {
    window.DbView.render(container, state, { doc: document, logic, lang, actions, t, format: ui ? ui.format : null });
  }

  function applyLang() {
    lang = currentLang();
    document.documentElement.lang = lang;
    document.title = logic.t(lang, "tab");
  }

  const controller = window.DbController.createController({ hangar: window.hangar, logic, lang, onChange: draw });

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
