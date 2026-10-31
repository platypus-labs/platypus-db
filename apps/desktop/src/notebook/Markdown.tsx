import { Fragment, type ReactNode } from "react";
import { type Block, type Inline, parseMarkdown } from "./html";

function inline(nodes: Inline[]): ReactNode {
  return nodes.map((n, i) => {
    switch (n.kind) {
      case "text":
        return <Fragment key={i}>{n.text}</Fragment>;
      case "code":
        return <code key={i}>{n.text}</code>;
      case "strong":
        return <strong key={i}>{inline(n.children)}</strong>;
      case "em":
        return <em key={i}>{inline(n.children)}</em>;
      // The app's window does not browse: the address shows on hover, and the export links it.
      case "link":
        return (
          <span key={i} className="notebook-link" title={n.href}>
            {inline(n.children)}
          </span>
        );
    }
  });
}

function block(b: Block, i: number): ReactNode {
  switch (b.kind) {
    case "heading": {
      const Tag = `h${b.level}` as const;
      return <Tag key={i}>{inline(b.children)}</Tag>;
    }
    case "paragraph":
      return <p key={i}>{inline(b.children)}</p>;
    case "list": {
      const Tag = b.ordered ? "ol" : "ul";
      return (
        <Tag key={i} start={b.ordered && b.start !== 1 ? b.start : undefined}>
          {b.items.map((item, j) => (
            <li key={j}>{inline(item)}</li>
          ))}
        </Tag>
      );
    }
    case "quote":
      return <blockquote key={i}>{b.children.map(block)}</blockquote>;
    case "code":
      return (
        <pre key={i}>
          <code>{b.text}</code>
        </pre>
      );
    case "rule":
      return <hr key={i} />;
  }
}

/** `text` rendered as Markdown; no part of it becomes markup. */
export function Markdown({ text }: { text: string }) {
  return <div className="notebook-markdown">{parseMarkdown(text).map(block)}</div>;
}
