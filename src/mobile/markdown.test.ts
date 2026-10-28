import { describe, expect, it } from "vitest";
import { parseInline, parseMarkdown, plainText, safeHref, type Block } from "./markdown";

const text = (value: string) => ({ type: "text", text: value });

describe("markdown blocks", () => {
  it("reads headings, paragraphs and rules, a line break kept as one", () => {
    expect(parseMarkdown("## The plan\n\nFirst line\nsecond line\n\n---\n# Done #")).toEqual([
      { type: "heading", level: 2, children: [text("The plan")] },
      { type: "paragraph", children: [text("First line"), { type: "break" }, text("second line")] },
      { type: "rule" },
      { type: "heading", level: 1, children: [text("Done")] },
    ]);
  });

  it("keeps a fenced block's text as written, with its language", () => {
    expect(parseMarkdown("Run:\n```bash\npnpm test\n  --run *.ts <b>\n```\nAfter")).toEqual([
      { type: "paragraph", children: [text("Run:")] },
      { type: "code", lang: "bash", text: "pnpm test\n  --run *.ts <b>" },
      { type: "paragraph", children: [text("After")] },
    ]);
  });

  it("runs an unclosed fence to the end of the message", () => {
    expect(parseMarkdown("~~~\nstill code")).toEqual([{ type: "code", lang: "", text: "still code" }]);
  });

  it("reads lists that interrupt a paragraph, nested ones and numbered ones", () => {
    const blocks = parseMarkdown("Checks:\n- tsc: clean\n- tests:\n  - 61 files\n  - 1 024 tests\n\n3. three\n4. four");
    expect(blocks[0]).toEqual({ type: "paragraph", children: [text("Checks:")] });
    const list = blocks[1] as Extract<Block, { type: "list" }>;
    expect(list.ordered).toBe(false);
    expect(list.items).toHaveLength(2);
    expect(list.items[1].children[1]).toMatchObject({
      type: "list",
      items: [{ children: [{ type: "paragraph", children: [text("61 files")] }] }, { children: [{ type: "paragraph" }] }],
    });
    expect(blocks[2]).toMatchObject({ type: "list", ordered: true, start: 3, items: [{}, {}] });
  });

  it("nests an item indented less than its parent's text, as Claude often writes", () => {
    const [list] = parseMarkdown("1. Server\n  - pairing\n  - cookie\n2. Phone");
    expect(list).toMatchObject({ type: "list", ordered: true, items: [{ children: [{}, { type: "list", ordered: false }] }, {}] });
  });

  it("carries a list item's paragraph on lines that lost their indent", () => {
    const [list] = parseMarkdown("- a long item\nthat wraps\n- next");
    expect(list).toMatchObject({
      items: [{ children: [{ type: "paragraph", children: [text("a long item"), { type: "break" }, text("that wraps")] }] }, {}],
    });
  });

  it("reads task list boxes", () => {
    const [list] = parseMarkdown("- [x] server\n- [ ] phone\n- plain");
    expect((list as Extract<Block, { type: "list" }>).items.map((item) => item.task)).toEqual([true, false, null]);
  });

  it("reads quotes, paragraphs within them", () => {
    expect(parseMarkdown("> first\n>\n> second")).toEqual([
      {
        type: "quote",
        children: [
          { type: "paragraph", children: [text("first")] },
          { type: "paragraph", children: [text("second")] },
        ],
      },
    ]);
  });

  it("reads tables with their alignment, pipes in code kept", () => {
    expect(parseMarkdown("| Check | Result |\n|:---|---:|\n| `a|b` | **ok** |\n| tests")).toEqual([
      {
        type: "table",
        align: ["left", "right"],
        head: [[text("Check")], [text("Result")]],
        rows: [
          [[{ type: "code", text: "a|b" }], [{ type: "strong", children: [text("ok")] }]],
          [[text("tests")], []],
        ],
      },
    ]);
  });

  it("leaves raw HTML as the text it is", () => {
    expect(parseMarkdown('<img src=x onerror="alert(1)">\n<script>alert(1)</script>')).toEqual([
      {
        type: "paragraph",
        children: [text('<img src=x onerror="alert(1)">'), { type: "break" }, text("<script>alert(1)</script>")],
      },
    ]);
  });
});

describe("markdown inline", () => {
  it("reads code, bold, italics and strikethrough, nested", () => {
    expect(parseInline("Run `pnpm test`, **then *ship* it**, ~~not~~ _now_")).toEqual([
      text("Run "),
      { type: "code", text: "pnpm test" },
      text(", "),
      { type: "strong", children: [text("then "), { type: "em", children: [text("ship")] }, text(" it")] },
      text(", "),
      { type: "del", children: [text("not")] },
      text(" "),
      { type: "em", children: [text("now")] },
    ]);
  });

  it("leaves lone stars, snake_case and escapes alone", () => {
    expect(parseInline("2 * 3 * 4, remote_permission_answer, \\*literal\\*")).toEqual([
      text("2 * 3 * 4, remote_permission_answer, *literal*"),
    ]);
  });

  it("keeps marks inside code as they are", () => {
    expect(parseInline("``a `b` *c*``")).toEqual([{ type: "code", text: "a `b` *c*" }]);
  });

  it("links http(s) and mailto addresses, and nothing else", () => {
    expect(parseInline("[the report](https://example.com/a_(b)) and [x](javascript:alert(1))")).toEqual([
      { type: "link", href: "https://example.com/a_(b)", children: [text("the report")] },
      text(" and x"),
    ]);
    expect(safeHref(" mailto:me@example.com")).toBe("mailto:me@example.com");
    expect(safeHref("data:text/html,<b>")).toBeNull();
    expect(safeHref("/relative")).toBeNull();
  });

  it("links bare addresses, without the punctuation that ends the sentence", () => {
    expect(parseInline("See https://hangar.simnjs.fr/docs. Or (https://example.com/x).")).toEqual([
      text("See "),
      { type: "link", href: "https://hangar.simnjs.fr/docs", children: [text("https://hangar.simnjs.fr/docs")] },
      text(". Or ("),
      { type: "link", href: "https://example.com/x", children: [text("https://example.com/x")] },
      text(")."),
    ]);
    expect(parseInline("<https://example.com>")).toEqual([
      { type: "link", href: "https://example.com", children: [text("https://example.com")] },
    ]);
  });

  it("never loads an image: its description, linked when the address may be", () => {
    expect(parseInline("![the For you screen](https://example.com/a.png) ![local](./a.png)")).toEqual([
      { type: "link", href: "https://example.com/a.png", children: [text("the For you screen")] },
      text(" local"),
    ]);
  });

  it("gives the text without its marks", () => {
    expect(plainText(parseInline("**Done**: `ok`\nnext"))).toBe("Done: ok\nnext");
  });
});
