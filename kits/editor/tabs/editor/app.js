"use strict";
// Starts the tab in the page: wires the controller to the view, in Hangar's language.
(function () {
  const logic = window.EditorLogic;
  const ui = window.hangar.ui;
  const t = ui
    ? ui.i18n(logic.STRINGS)
    : (key, vars) => logic.t(logic.pickLang(navigator.language), key, vars);
  const language = () => (ui ? ui.locale() : logic.pickLang(navigator.language));
  const container = document.getElementById("app");

  let refresh = () => undefined;
  const controller = window.EditorController.createController({
    hangar: window.hangar,
    t,
    lib: {
      logic,
      dotenv: window.EditorDotenv,
      validate: window.EditorValidate,
      diff: window.EditorDiff,
      gitignore: window.EditorGitignore,
      answer: window.EditorAnswer,
    },
    onChange: draw,
    onTyping: () => refresh(),
  });

  const ctx = {
    doc: document,
    t,
    logic,
    gitignore: window.EditorGitignore,
    actions: controller,
    confirm: (anchor, options) => (ui ? ui.confirm(anchor, options) : Promise.resolve(false)),
  };

  function draw(state) {
    document.documentElement.lang = language();
    document.title = t("tab");
    refresh = window.EditorView.render(container, state, ctx);
  }

  // The first `theme` event may bring Hangar's language; a later one may change it.
  let lastLanguage = language();
  window.hangar.on("theme", () => {
    if (language() === lastLanguage) return;
    lastLanguage = language();
    draw(controller.state);
  });

  draw(controller.state);
  controller.start();
})();
