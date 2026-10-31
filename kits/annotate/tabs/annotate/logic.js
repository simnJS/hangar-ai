"use strict";
// What the tab says and the small rules around it: its words in English and French,
// the params, the names of the files it writes, the zoom. No DOM, no window.hangar.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.AnnotateLogic = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  /** Where the image and its notes are written, relative to the workspace folder. */
  const FOLDER = ".hangar/annotations";
  /** What a PNG written by `files.writeBinary` may weigh once encoded in base64 (10 MB of bytes). */
  const BASE64_MAX = Math.ceil((10 * 1024 * 1024) / 3) * 4;
  /** The largest image file read from disk, pasted or dropped. */
  const INPUT_MAX_BYTES = 30 * 1024 * 1024;
  /** Images the tab opens: what a canvas decodes, never an SVG (script) or a PDF. */
  const OPENABLE = ["image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp", "image/avif"];
  /** The scales an image is tried at when its PNG is too heavy, until it fits. */
  const EXPORT_SCALES = [1, 0.75, 0.5, 0.35, 0.25];
  const ZOOM_STEPS = [0.1, 0.15, 0.25, 0.33, 0.5, 0.67, 0.75, 1, 1.5, 2, 3, 4];
  const NOTE_MAX = 500;
  const COMMENT_MAX = 2000;
  const PANE_MAX = 200;

  const STRINGS = {
    en: {
      tab: "Annotate",
      tools: "Tools",
      tool_arrow: "Arrow",
      tool_rect: "Rectangle",
      tool_free: "Freehand",
      tool_note: "Note",
      tip_arrow: "Drag on the image to point at something.",
      tip_rect: "Drag on the image to frame something.",
      tip_free: "Draw on the image with the pointer.",
      tip_note: "Click where the problem is, then write a word.",
      color: "Color",
      color_red: "Red",
      color_orange: "Orange",
      color_green: "Green",
      color_blue: "Blue",
      color_magenta: "Magenta",
      undo: "Undo",
      zoom: "Zoom",
      zoom_in: "Zoom in",
      zoom_out: "Zoom out",
      zoom_fit: "Fit",
      zoom_value: "{n} %",
      canvas: "The image to annotate",
      image_size: "{w} × {h} px",
      another: "Another image",
      another_ask: "Replace this image and drop its marks?",
      another_yes: "Replace",
      another_no: "Keep",
      empty_title: "No image yet",
      empty_text: "Paste a capture with Ctrl+V, drop an image here, or choose a file.",
      choose: "Choose an image",
      loading: "Reading {path}…",
      denied_title: "This tab may not read files",
      denied_text: "Accept files.read in Settings › Kits to open an image by its path. You can still paste or drop one.",
      not_found: "{path} was not found in the workspace folder.",
      unsupported: "{path} is not a PNG, JPEG, WebP or GIF image.",
      read_failed: "Could not read {path}: {reason}",
      bad_image: "This is not an image the tab can open. Use a PNG, JPEG, WebP or GIF.",
      too_large: "This image is larger than 30 MB.",
      keep_marks: "Marks are on this image. Use « Another image » to replace it.",
      notes_title: "Notes",
      note_label: "Note {n}",
      note_placeholder: "What is wrong here?",
      no_notes: "Notes you place on the image are listed here.",
      comment_label: "Comment (optional)",
      comment_placeholder: "What should the agent do?",
      target_label: "Send to",
      target_placeholder: "Pane name",
      target_caller: "{pane} asked for this image.",
      target_none: "Without a pane name, the files are only saved.",
      send: "Send",
      save: "Save",
      sending: "Sending…",
      sent: "Sent to {pane}: {png}",
      saved: "Saved: {png}",
      write_denied: "This tab may not write files. Accept files.write in Settings › Kits.",
      write_unknown: "This Hangar cannot save images yet. Update Hangar, then try again.",
      write_failed: "Could not save the image: {reason}",
      json_failed: "The image is saved, but not its notes file: {reason}",
      send_denied: "Saved: {png}. This tab may not tell an agent: accept panes.send in Settings › Kits.",
      send_not_found: "Saved: {png}. No pane is named {pane} in this workspace.",
      send_unavailable: "Saved: {png}. {pane} cannot take a message right now.",
      send_failed: "Saved: {png}. {pane} was not told: {reason}",
      too_heavy: "The image is too heavy to save, even reduced.",
      retry: "Try again",
    },
    fr: {
      tab: "Annoter",
      tools: "Outils",
      tool_arrow: "Flèche",
      tool_rect: "Rectangle",
      tool_free: "Main levée",
      tool_note: "Note",
      tip_arrow: "Faites glisser sur l’image pour pointer quelque chose.",
      tip_rect: "Faites glisser sur l’image pour entourer quelque chose.",
      tip_free: "Dessinez sur l’image avec le pointeur.",
      tip_note: "Cliquez là où est le problème, puis écrivez un mot.",
      color: "Couleur",
      color_red: "Rouge",
      color_orange: "Orange",
      color_green: "Vert",
      color_blue: "Bleu",
      color_magenta: "Magenta",
      undo: "Annuler",
      zoom: "Zoom",
      zoom_in: "Zoom avant",
      zoom_out: "Zoom arrière",
      zoom_fit: "Ajuster",
      zoom_value: "{n} %",
      canvas: "L’image à annoter",
      image_size: "{w} × {h} px",
      another: "Autre image",
      another_ask: "Remplacer cette image et perdre ses marques ?",
      another_yes: "Remplacer",
      another_no: "Garder",
      empty_title: "Pas encore d’image",
      empty_text: "Collez une capture avec Ctrl+V, déposez une image ici ou choisissez un fichier.",
      choose: "Choisir une image",
      loading: "Lecture de {path}…",
      denied_title: "Cet onglet ne peut pas lire de fichiers",
      denied_text: "Acceptez files.read dans Réglages › Kits pour ouvrir une image par son chemin. Vous pouvez toujours en coller ou en déposer une.",
      not_found: "{path} est introuvable dans le dossier du workspace.",
      unsupported: "{path} n’est pas une image PNG, JPEG, WebP ou GIF.",
      read_failed: "Impossible de lire {path} : {reason}",
      bad_image: "Cette image ne peut pas être ouverte. Utilisez un PNG, un JPEG, un WebP ou un GIF.",
      too_large: "Cette image dépasse 30 Mo.",
      keep_marks: "Des marques sont posées sur cette image. Utilisez « Autre image » pour la remplacer.",
      notes_title: "Notes",
      note_label: "Note {n}",
      note_placeholder: "Qu’est-ce qui ne va pas ici ?",
      no_notes: "Les notes posées sur l’image sont listées ici.",
      comment_label: "Commentaire (facultatif)",
      comment_placeholder: "Que doit faire l’agent ?",
      target_label: "Envoyer à",
      target_placeholder: "Nom du pane",
      target_caller: "{pane} a demandé cette image.",
      target_none: "Sans nom de pane, les fichiers sont seulement enregistrés.",
      send: "Envoyer",
      save: "Enregistrer",
      sending: "Envoi…",
      sent: "Envoyé à {pane} : {png}",
      saved: "Enregistré : {png}",
      write_denied: "Cet onglet ne peut pas écrire de fichiers. Acceptez files.write dans Réglages › Kits.",
      write_unknown: "Cette version de Hangar ne sait pas encore enregistrer d’images. Mettez Hangar à jour, puis réessayez.",
      write_failed: "Impossible d’enregistrer l’image : {reason}",
      json_failed: "L’image est enregistrée, pas son fichier de notes : {reason}",
      send_denied: "Enregistré : {png}. Cet onglet ne peut pas prévenir un agent : acceptez panes.send dans Réglages › Kits.",
      send_not_found: "Enregistré : {png}. Aucun pane ne s’appelle {pane} dans ce workspace.",
      send_unavailable: "Enregistré : {png}. {pane} ne peut pas recevoir de message pour l’instant.",
      send_failed: "Enregistré : {png}. {pane} n’a pas été prévenu : {reason}",
      too_heavy: "L’image est trop lourde pour être enregistrée, même réduite.",
      retry: "Réessayer",
    },
  };

  /** "fr", "fr-FR" → "fr"; anything else is English. */
  function pickLang(locale) {
    return typeof locale === "string" && /^fr(?:[-_]|$)/i.test(locale.trim()) ? "fr" : "en";
  }

  /** Without Hangar UI's `i18n`: the same lookup, English filling in. */
  function t(lang, key, vars) {
    const template = (STRINGS[lang] && STRINGS[lang][key]) || STRINGS.en[key] || key;
    return template.replace(/\{(\w+)\}/g, (match, name) => (vars && vars[name] !== undefined ? String(vars[name]) : match));
  }

  /** The `path` param: a non-empty string of a sane length, else null. The host checks the rest. */
  function cleanPath(value) {
    if (typeof value !== "string") return null;
    const path = value.trim();
    return path && path.length <= 1024 ? path : null;
  }

  function baseName(path) {
    const parts = String(path).split(/[\\/]/);
    return parts[parts.length - 1] || String(path);
  }

  /** `20261031-231512`, from local time: the name a pair of files shares. */
  function stamp(date) {
    const pad = (n, size = 2) => String(n).padStart(size, "0");
    return (
      `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}` +
      `-${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`
    );
  }

  /** The names tried for one send: `stamp`, then `stamp-2`… when a file is already there. */
  function names(base, tries = 5) {
    return Array.from({ length: tries }, (_, i) => (i === 0 ? base : `${base}-${i + 1}`));
  }

  function openableMime(mime) {
    return OPENABLE.includes(String(mime).toLowerCase());
  }

  function dataUrl(mime, base64) {
    return `data:${String(mime).toLowerCase()};base64,${base64}`;
  }

  /** The zoom that shows the whole image in the box, never above 100 %. */
  function fitZoom(width, height, boxWidth, boxHeight) {
    if (!(width > 0) || !(height > 0) || !(boxWidth > 0)) return 1;
    const byWidth = boxWidth / width;
    const byHeight = boxHeight > 0 ? boxHeight / height : byWidth;
    return Math.min(1, byWidth, byHeight);
  }

  /** The next step above or below `current` among ZOOM_STEPS, or `current` at the end. */
  function stepZoom(current, direction) {
    const eps = 1e-6;
    if (direction > 0) return ZOOM_STEPS.find((step) => step > current + eps) || ZOOM_STEPS[ZOOM_STEPS.length - 1];
    const below = ZOOM_STEPS.filter((step) => step < current - eps);
    return below.length ? below[below.length - 1] : ZOOM_STEPS[0];
  }

  /** At most `max` characters, counted in code points, no line breaks in a note. */
  function clip(text, max, keepBreaks) {
    const flat = keepBreaks ? String(text) : String(text).replace(/\s*[\r\n]+\s*/g, " ");
    const chars = Array.from(flat);
    return chars.length > max ? chars.slice(0, max).join("") : flat;
  }

  return {
    FOLDER,
    BASE64_MAX,
    INPUT_MAX_BYTES,
    OPENABLE,
    EXPORT_SCALES,
    ZOOM_STEPS,
    NOTE_MAX,
    COMMENT_MAX,
    PANE_MAX,
    STRINGS,
    pickLang,
    t,
    cleanPath,
    baseName,
    stamp,
    names,
    openableMime,
    dataUrl,
    fitZoom,
    stepZoom,
    clip,
  };
});
