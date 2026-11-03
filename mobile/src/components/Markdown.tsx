import { memo, useMemo, type ReactNode } from "react";
import { parseMarkdown, type Block, type Inline } from "../markdown";

/**
 * Claude's markdown, drawn from the tree markdown.ts reads: React only ever
 * gets text, so no message can put HTML on the page. A code block or a table
 * scrolls sideways inside itself, never the chat.
 */
export const Markdown = memo(function Markdown({ text, className }: { text: string; className?: string }) {
  const blocks = useMemo(() => parseMarkdown(text), [text]);
  return <div className={className ? `md ${className}` : "md"}>{blocks.map(block)}</div>;
});

function block(node: Block, index: number): ReactNode {
  switch (node.type) {
    case "heading": {
      // A message's headings sit under the screen's own: h3 and below.
      const Tag = (["h3", "h4", "h5", "h6"] as const)[Math.min(node.level, 4) - 1];
      return (
        <Tag key={index} className={`md__h md__h${Math.min(node.level, 4)}`}>
          {node.children.map(inline)}
        </Tag>
      );
    }
    case "paragraph":
      return <p key={index}>{node.children.map(inline)}</p>;
    case "code":
      return (
        <pre key={index} className="md__pre" tabIndex={0} data-lang={node.lang || undefined}>
          <code>{node.text}</code>
        </pre>
      );
    case "quote":
      return <blockquote key={index}>{node.children.map(block)}</blockquote>;
    case "list": {
      const items = node.items.map((item, at) => (
        <li key={at} className={item.task === null ? undefined : "md__task"}>
          {item.task !== null && <input type="checkbox" className="md__box" checked={item.task} disabled readOnly />}
          {item.children.map(block)}
        </li>
      ));
      return node.ordered ? (
        <ol key={index} start={node.start === 1 ? undefined : node.start}>
          {items}
        </ol>
      ) : (
        <ul key={index}>{items}</ul>
      );
    }
    case "table":
      return (
        <div key={index} className="md__table" tabIndex={0}>
          <table>
            <thead>
              <tr>
                {node.head.map((cell, column) => (
                  <th key={column} style={node.align[column] ? { textAlign: node.align[column]! } : undefined}>
                    {cell.map(inline)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {node.rows.map((row, at) => (
                <tr key={at}>
                  {row.map((cell, column) => (
                    <td key={column} style={node.align[column] ? { textAlign: node.align[column]! } : undefined}>
                      {cell.map(inline)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "rule":
      return <hr key={index} />;
  }
}

function inline(node: Inline, index: number): ReactNode {
  switch (node.type) {
    case "text":
      return node.text;
    case "code":
      return (
        <code key={index} className="md__code">
          {node.text}
        </code>
      );
    case "strong":
      return <strong key={index}>{node.children.map(inline)}</strong>;
    case "em":
      return <em key={index}>{node.children.map(inline)}</em>;
    case "del":
      return <del key={index}>{node.children.map(inline)}</del>;
    case "link":
      return (
        <a key={index} className="md__link" href={node.href} target="_blank" rel="noopener noreferrer">
          {node.children.map(inline)}
        </a>
      );
    case "break":
      return <br key={index} />;
  }
}
