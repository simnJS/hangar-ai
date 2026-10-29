import { describe, expect, it } from "vitest";
import { renderMarkdown } from "../components/preview/markdown";
import { highlightLines } from "../components/preview/highlight";
import { modelUrl } from "./modelFiles";
import {
  checkPreviewFile,
  escapeHtml,
  fitZoom,
  hasSource,
  headingId,
  languageOf,
  markdownImageSrc,
  markdownLink,
  mimeType,
  pdfInline,
  prettyJson,
  PREVIEW_KINDS,
  previewUrl,
  rangeTotal,
  splitHighlighted,
  textHead,
  textLines,
  viewerOf,
  zoomAround,
  ZOOM_MAX,
} from "./preview";

describe("from preview_inspect to a viewer", () => {
  const inspected = (kind: string, patch: Record<string, unknown> = {}) => ({
    path: `C:\\dev\\out\\file.${kind}`,
    kind,
    mime: "x/y",
    size: 12,
    name: `file.${kind}`,
    modifiedMs: 1700000000000,
    ...patch,
  });

  it("draws every kind Rust reports with its own viewer", () => {
    const viewers = PREVIEW_KINDS.map((kind) => [kind, viewerOf(checkPreviewFile(inspected(kind)).kind)]);
    expect(viewers).toEqual([
      ["model", "model"],
      ["image", "image"],
      ["video", "media"],
      ["audio", "media"],
      ["text", "code"],
      ["markdown", "markdown"],
      ["pdf", "pdf"],
      ["html", "html"],
    ]);
  });

  it("keeps what Rust said, and names a file it gave no name", () => {
    expect(checkPreviewFile(inspected("pdf"))).toEqual({
      path: "C:\\dev\\out\\file.pdf",
      kind: "pdf",
      mime: "x/y",
      size: 12,
      name: "file.pdf",
      modifiedMs: 1700000000000,
    });
    expect(
      checkPreviewFile(inspected("image", { name: undefined, size: -3, mime: 4, modifiedMs: "now" })),
    ).toMatchObject({ name: "file.image", size: 0, mime: "", modifiedMs: 0 });
  });

  it("refuses a kind no viewer draws, and an answer without a path", () => {
    expect(() => checkPreviewFile(inspected("binary"))).toThrow(
      /no viewer for file\.binary \(binary\)\. It shows: model, image, video, audio, text, markdown, pdf, html/,
    );
    expect(() => checkPreviewFile({ kind: "image" })).toThrow(/no path/);
    expect(() => checkPreviewFile(null)).toThrow(/no path/);
  });

  it("says what a file is by its type alone, and has a source to show for SVG", () => {
    expect(mimeType("text/plain; charset=utf-8")).toBe("text/plain");
    expect(mimeType("image/png")).toBe("image/png");
    expect(hasSource("C:\\dev\\art\\logo.SVG")).toBe(true);
    expect(hasSource("C:\\dev\\art\\logo.png")).toBe(false);
  });

  it("loads a file again under a new address each time it is shown again, its source as text", () => {
    const base = modelUrl("C:\\dev\\site\\index.html");
    expect(previewUrl("C:\\dev\\site\\index.html", 0)).toBe(base);
    expect(previewUrl("C:\\dev\\site\\index.html", 2)).toBe(`${base}?rev=2`);
    expect(previewUrl("C:\\dev\\site\\index.html", 0, true)).toBe(`${base}?as=text`);
    expect(previewUrl("C:\\dev\\site\\index.html", 3, true)).toBe(`${base}?as=text&rev=3`);
  });
});

describe("text and code", () => {
  it("colours a file by its extension or its name, and leaves plain text plain", () => {
    expect(languageOf("C:\\dev\\src\\App.tsx")).toBe("typescript");
    expect(languageOf("/home/me/Cargo.toml")).toBe("ini");
    expect(languageOf("data.JSON")).toBe("json");
    expect(languageOf("events.jsonl")).toBe("json");
    expect(languageOf("site/index.html")).toBe("xml");
    expect(languageOf("docker/Dockerfile")).toBe("dockerfile");
    expect(languageOf(".env")).toBe("ini");
    expect(languageOf("notes.txt")).toBeNull();
    expect(languageOf("LICENSE")).toBeNull();
    expect(languageOf(".gitignore")).toBeNull();
  });

  it("keeps a whole file whole, without its byte-order mark or Windows line ends", () => {
    expect(textHead("\uFEFFone\r\ntwo\r\n", false)).toEqual({ text: "one\ntwo\n", truncated: false });
  });

  it("drops the piece of a line a cut leaves, unless it is the only line", () => {
    expect(textHead("one\ntwo\nthr", true)).toEqual({ text: "one\ntwo", truncated: true });
    expect(textHead('{"minified":true,"and":"cut', true)).toEqual({
      text: '{"minified":true,"and":"cut',
      truncated: true,
    });
  });

  it("stops at the line cap, and says the file goes on", () => {
    const lines = Array.from({ length: 12 }, (_, i) => `line ${i + 1}`).join("\n");
    const head = textHead(lines, false, 10);
    expect(head.truncated).toBe(true);
    expect(textLines(head.text)).toHaveLength(10);
    expect(textHead("a\nb", false, 10).truncated).toBe(false);
  });

  it("does not draw an empty row for a final line break", () => {
    expect(textLines("a\nb\n")).toEqual(["a", "b"]);
    expect(textLines("a\n\n")).toEqual(["a", ""]);
    expect(textLines("")).toEqual([""]);
  });

  it("reads the whole file's size from a Content-Range", () => {
    expect(rangeTotal("bytes 0-262143/1048576")).toBe(1048576);
    expect(rangeTotal("bytes */0")).toBe(0);
    expect(rangeTotal(null)).toBeNull();
  });

  it("lays out JSON written on one line, keeping numbers and strings as they are", () => {
    const pretty = prettyJson('{"id":12345678901234567890,"tags":["a,b","{c}"],"empty":{},"none":[],"q":"say \\"hi\\""}');
    expect(pretty).toBe(
      [
        "{",
        '  "id": 12345678901234567890,',
        '  "tags": [',
        '    "a,b",',
        '    "{c}"',
        "  ],",
        '  "empty": {},',
        '  "none": [],',
        '  "q": "say \\"hi\\""',
        "}",
      ].join("\n"),
    );
  });

  it("leaves JSON its author laid out, JSON Lines, invalid JSON and bare values as written", () => {
    expect(prettyJson('{\n    "a": 1\n}')).toBeNull();
    expect(prettyJson('{"a":1}\n{"a":2}')).toBeNull();
    expect(prettyJson('{"a":1')).toBeNull();
    expect(prettyJson("42")).toBeNull();
    expect(prettyJson("")).toBeNull();
  });

  it("cuts highlighted HTML into whole lines, a span across a break closed and opened again", () => {
    const html = 'a<span class="c">/* one\ntwo */</span>\n<span class="k">b</span>';
    expect(splitHighlighted(html)).toEqual([
      'a<span class="c">/* one</span>',
      '<span class="c">two */</span>',
      '<span class="k">b</span>',
    ]);
    expect(splitHighlighted("x\n")).toEqual(["x", ""]);
  });

  it("highlights every line of a file, a block comment's included, its markup escaped", () => {
    const lines = highlightLines("/* a\n<b> */\nconst x = 1;", "ts")!;
    expect(lines).toHaveLength(3);
    expect(lines[1]).toMatch(/^<span class="hljs-comment">&lt;b&gt; \*\/<\/span>$/);
    expect(lines[2]).toContain('<span class="hljs-keyword">const</span>');
    expect(highlightLines("plain", "klingon")).toBeNull();
  });

  it("escapes what it draws as text", () => {
    expect(escapeHtml(`<a href="x">'&'</a>`)).toBe("&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
  });
});

describe("Markdown", () => {
  const fileUrl = modelUrl("C:\\dev\\docs\\README.md");

  it("loads a relative image from beside the file, a disk path through the scheme, a web one as it is", () => {
    expect(markdownImageSrc("img/a b.png", fileUrl)).toBe(modelUrl("C:\\dev\\docs\\img\\a b.png"));
    expect(markdownImageSrc("../art/logo.svg", fileUrl)).toBe(modelUrl("C:\\dev\\art\\logo.svg"));
    expect(markdownImageSrc("C:\\shots\\one.png", fileUrl)).toBe(modelUrl("C:\\shots\\one.png"));
    expect(markdownImageSrc("https://example.com/badge.svg", fileUrl)).toBe("https://example.com/badge.svg");
    expect(markdownImageSrc("//cdn.example.com/a.png", fileUrl)).toBe("https://cdn.example.com/a.png");
    expect(markdownImageSrc("data:image/png;base64,AAAA", fileUrl)).toBe("data:image/png;base64,AAAA");
  });

  it("loads nothing from another scheme", () => {
    expect(markdownImageSrc("javascript:alert(1)", fileUrl)).toBeNull();
    expect(markdownImageSrc("file:///C:/secret.png", fileUrl)).toBeNull();
    expect(markdownImageSrc("data:text/html;base64,PHNjcmlwdD4=", fileUrl)).toBeNull();
    expect(markdownImageSrc("  ", fileUrl)).toBeNull();
  });

  it("scrolls to a heading, opens the web outside, and goes nowhere for anything else", () => {
    expect(markdownLink("#setting-up")).toEqual({ kind: "anchor", id: "setting-up" });
    expect(markdownLink("#caf%C3%A9")).toEqual({ kind: "anchor", id: "café" });
    expect(markdownLink("https://hangar.simnjs.fr")).toEqual({ kind: "external", url: "https://hangar.simnjs.fr" });
    expect(markdownLink("mailto:me@example.com")).toEqual({ kind: "external", url: "mailto:me@example.com" });
    expect(markdownLink("docs/other.md")).toEqual({ kind: "none" });
    expect(markdownLink("javascript:alert(1)")).toEqual({ kind: "none" });
    expect(markdownLink("#")).toEqual({ kind: "none" });
  });

  it("gives headings GitHub's ids, numbered when a title comes back", () => {
    const taken = new Map<string, number>();
    expect(headingId("Setting up", taken)).toBe("setting-up");
    expect(headingId("Setting up", taken)).toBe("setting-up-1");
    expect(headingId("What's new? (v1.2)", taken)).toBe("whats-new-v12");
    expect(headingId("Café crème", taken)).toBe("café-crème");
    expect(headingId("!!!", taken)).toBe("section");
  });

  it("never turns the file's own HTML into markup", () => {
    const html = renderMarkdown(
      '<script>alert(1)</script>\n\n<img src=x onerror="alert(1)">\n\n[click](javascript:alert(1))',
      fileUrl,
    );
    expect(html).not.toMatch(/<script|<img|onerror="|href="javascript/i);
    expect(html).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
  });

  it("resolves images, numbers headings and colours fenced code as it renders", () => {
    const html = renderMarkdown("# Intro\n\n![shot](shots/one.png)\n\n```ts\nconst a = 1;\n```\n", fileUrl);
    expect(html).toContain('<h1 id="intro">Intro</h1>');
    expect(html).toContain(`src="${modelUrl("C:\\dev\\docs\\shots\\one.png")}"`);
    expect(html).toContain('<span class="hljs-keyword">const</span>');
  });

  it("shows an image it may not load as its alt text", () => {
    expect(renderMarkdown("![the <b>logo</b>](file:///C:/logo.png)", fileUrl)).not.toContain("<img");
  });
});

describe("PDF", () => {
  it("is drawn in the panel only where the webview says it has a viewer", () => {
    expect(pdfInline({ pdfViewerEnabled: true })).toBe(true);
    expect(pdfInline({ pdfViewerEnabled: false })).toBe(false);
    expect(pdfInline({})).toBe(false);
    expect(pdfInline(undefined)).toBe(false);
  });
});

describe("image zoom", () => {
  it("fits an image inside the stage with a margin, never past its own pixels", () => {
    expect(fitZoom({ width: 2000, height: 1000 }, { width: 1032, height: 800 })).toBe(0.5);
    expect(fitZoom({ width: 64, height: 64 }, { width: 800, height: 600 })).toBe(1);
    expect(fitZoom({ width: 0, height: 0 }, { width: 800, height: 600 })).toBe(1);
  });

  it("keeps the spot under the pointer where it is, within the limits", () => {
    const zoomed = zoomAround({ zoom: 1, x: 0, y: 0 }, 2, { x: 100, y: -50 });
    expect(zoomed).toEqual({ zoom: 2, x: -100, y: 50 });
    // The image point that was under the pointer is under it still.
    const imagePoint = (frame: typeof zoomed, p: { x: number; y: number }) => ({
      x: (p.x - frame.x) / frame.zoom,
      y: (p.y - frame.y) / frame.zoom,
    });
    expect(imagePoint(zoomed, { x: 100, y: -50 })).toEqual(imagePoint({ zoom: 1, x: 0, y: 0 }, { x: 100, y: -50 }));
    expect(zoomAround({ zoom: 20, x: 0, y: 0 }, 10).zoom).toBe(ZOOM_MAX);
  });
});
