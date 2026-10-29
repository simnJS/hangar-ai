import MarkdownIt, { type Env } from "markdown-it";
import { escapeHtml, headingId, markdownImageSrc } from "../../lib/preview";
import { highlight } from "./highlight";

/**
 * Markdown, rendered for the preview: markdown-it with `html: false`, so a
 * `<script>` or an `onerror=` in the file is shown as the text it is and
 * never becomes markup; its own link check keeps `javascript:` and friends
 * out of every href and src. Code blocks are coloured by ./highlight, images
 * load from beside the file, headings get the ids GitHub gives them so a
 * README's table of contents works. What a click on a link does is the
 * viewer's business (MarkdownView, lib/preview › markdownLink).
 *
 * Loaded on demand with the highlighter, never in the main bundle.
 */

interface PreviewEnv extends Env {
  /** The URL the file itself is served at, which relative images are read from. */
  fileUrl: string;
  /** Heading ids given so far, for the -1, -2 of a repeated title. */
  ids: Map<string, number>;
}

const md = new MarkdownIt({
  html: false,
  linkify: true,
  typographer: false,
  highlight: (code, info) => {
    const language = info.trim().split(/\s+/)[0] ?? "";
    const html = highlight(code, language);
    // Empty: markdown-it escapes the code and wraps it itself.
    return html === null
      ? ""
      : `<pre class="hljs"><code class="language-${escapeHtml(language)}">${html}</code></pre>`;
  },
});

const renderToken = md.renderer.renderToken.bind(md.renderer);

md.renderer.rules.image = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  const { fileUrl } = env as PreviewEnv;
  const src = markdownImageSrc(String(token.attrGet("src") ?? ""), fileUrl);
  const alt = self.renderInlineAsText(token.children ?? [], options, env);
  // An image that may not load is its alt text, rather than a broken frame.
  if (!src) return escapeHtml(alt);
  token.attrSet("src", src);
  token.attrSet("alt", alt);
  token.attrSet("loading", "lazy");
  return renderToken(tokens, idx, options);
};

md.renderer.rules.heading_open = (tokens, idx, options, env) => {
  const inline = tokens[idx + 1];
  const text = inline?.children
    ? inline.children
        .map((child) => (child.type === "text" || child.type === "code_inline" ? child.content : ""))
        .join("")
    : (inline?.content ?? "");
  tokens[idx].attrSet("id", headingId(text, (env as PreviewEnv).ids));
  return renderToken(tokens, idx, options);
};

/** `source` as safe HTML, its relative images resolved against `fileUrl`. */
export function renderMarkdown(source: string, fileUrl: string): string {
  const env: PreviewEnv = { fileUrl, ids: new Map() };
  return md.render(source, env);
}
