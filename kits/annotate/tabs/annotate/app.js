"use strict";
// Starts the tab in the page: wires the controller to the view, in Hangar's language,
// and does the three things only a browser can: decode an image, read a dropped file,
// paint the PNG that is written.
(function () {
  const logic = window.AnnotateLogic;
  const model = window.AnnotateModel;
  const ui = window.hangar.ui;
  const t = ui
    ? ui.i18n(logic.STRINGS)
    : (key, vars) => logic.t(logic.pickLang(navigator.language), key, vars);
  const language = () => (ui ? ui.locale() : logic.pickLang(navigator.language));
  const container = document.getElementById("app");
  const picker = document.getElementById("file");

  /** The decoded image by its URL: the canvas draws from it. */
  const images = new Map();

  const deps = {
    decode: (url) =>
      new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => {
          if (!image.naturalWidth || !image.naturalHeight) return reject(new Error("empty image"));
          images.clear();
          images.set(url, image);
          resolve({ width: image.naturalWidth, height: image.naturalHeight });
        };
        image.onerror = () => reject(new Error("not an image"));
        image.src = url;
      }),
    readFile: (file) =>
      new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(reader.error || new Error("unreadable"));
        reader.readAsDataURL(file);
      }),
    exportPng: async (state, scale) => {
      const width = Math.max(1, Math.round(state.image.width * scale));
      const height = Math.max(1, Math.round(state.image.height * scale));
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      const context = canvas.getContext("2d");
      if (!context) throw new Error("no canvas");
      const spec = { image: images.get(state.image.url), width: state.image.width, height: state.image.height, shapes: state.shapes };
      model.draw(context, spec, scale);
      const url = canvas.toDataURL("image/png");
      const head = "data:image/png;base64,";
      if (!url.startsWith(head)) throw new Error("the image is too large to paint");
      return url.slice(head.length);
    },
  };

  let refresh = () => undefined;
  const controller = window.AnnotateController.createController({
    hangar: window.hangar,
    t,
    lib: { logic, model, answer: window.AnnotateAnswer },
    deps,
    onChange: draw,
    onTyping: () => refresh(),
  });

  const ctx = {
    doc: document,
    win: window,
    t,
    logic,
    model,
    actions: controller,
    images: (url) => images.get(url) || null,
    pickFile: () => picker.click(),
    confirm: (anchor, options) => (ui ? ui.confirm(anchor, options) : Promise.resolve(false)),
  };

  function draw(state) {
    document.documentElement.lang = language();
    document.title = t("tab");
    refresh = window.AnnotateView.render(container, state, ctx);
  }

  picker.addEventListener("change", () => {
    const file = picker.files && picker.files[0];
    picker.value = "";
    if (file) controller.openFile(file);
  });

  document.addEventListener("paste", (event) => {
    const items = (event.clipboardData && event.clipboardData.items) || [];
    for (const item of items) {
      if (item.kind === "file" && item.type.startsWith("image/")) {
        const file = item.getAsFile();
        if (file) {
          event.preventDefault();
          controller.openFile(file);
          return;
        }
      }
    }
  });
  document.addEventListener("dragover", (event) => event.preventDefault());
  document.addEventListener("drop", (event) => {
    event.preventDefault();
    const file = event.dataTransfer && event.dataTransfer.files && event.dataTransfer.files[0];
    if (file) controller.openFile(file);
  });
  document.addEventListener("keydown", (event) => {
    const target = event.target;
    const typing = target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA");
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && String(event.key).toLowerCase() === "z" && !typing) {
      event.preventDefault();
      controller.undo();
    }
  });

  if (typeof ResizeObserver === "function") new ResizeObserver(() => refresh.layout && refresh.layout()).observe(container);

  // The first `theme` event may bring Hangar's language; a later one may change it.
  let lastLanguage = language();
  window.hangar.on("theme", () => {
    if (language() === lastLanguage) return;
    lastLanguage = language();
    draw(controller.state);
  });
  // An agent asked for this tab again: it is the one to answer now.
  window.hangar.on("tab.params", (data) => controller.receive(data));

  draw(controller.state);
  controller.start();
})();
