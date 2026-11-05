import { Fragment, type ReactNode, useMemo } from "react";
import { type Block, type Inline, parseMarkdown } from "./markdown";
import { classifyLink, imagePath, type LinkTarget } from "./pages";

interface Context {
  /** The page being shown: relative links and images are read from there. */
  page: string;
  images: Record<string, string>;
  onLink: (target: LinkTarget) => void;
}

function inline(nodes: Inline[], cx: Context): ReactNode {
  return nodes.map((n, i) => {
    switch (n.kind) {
      case "text":
        return <Fragment key={i}>{n.text}</Fragment>;
      case "code":
        return <code key={i}>{n.text}</code>;
      case "strong":
        return <strong key={i}>{inline(n.children, cx)}</strong>;
      case "em":
        return <em key={i}>{inline(n.children, cx)}</em>;
      case "image": {
        const path = imagePath(cx.page, n.src);
        const url = path ? cx.images[path] : undefined;
        // An image the app does not carry (a GIF, an address) is left out; its description stays.
        return url ? <img key={i} src={url} alt={n.alt} loading="lazy" /> : n.alt ? <span key={i} className="guide-no-image">{n.alt}</span> : null;
      }
      case "link": {
        const target = classifyLink(cx.page, n.href);
        // Never a real address: the window follows links through the dialog, or not at all.
        if (target.kind === "none") return <Fragment key={i}>{inline(n.children, cx)}</Fragment>;
        return (
          <a
            key={i}
            href="#guide-link"
            data-href={n.href}
            title={n.href}
            onClick={(e) => {
              e.preventDefault();
              cx.onLink(target);
            }}
          >
            {inline(n.children, cx)}
          </a>
        );
      }
    }
  });
}

function block(b: Block, i: number, cx: Context): ReactNode {
  switch (b.kind) {
    case "heading": {
      const Tag = `h${b.level}` as "h1";
      return (
        <Tag key={i} data-anchor={b.anchor}>
          {inline(b.children, cx)}
        </Tag>
      );
    }
    case "paragraph":
      return <p key={i}>{inline(b.children, cx)}</p>;
    case "list": {
      const Tag = b.ordered ? "ol" : "ul";
      return (
        <Tag key={i} start={b.ordered && b.start !== 1 ? b.start : undefined}>
          {b.items.map((item, j) => (
            <li key={j}>{item.map((child, k) => block(child, k, cx))}</li>
          ))}
        </Tag>
      );
    }
    case "quote":
      return <blockquote key={i}>{b.children.map((child, k) => block(child, k, cx))}</blockquote>;
    case "code":
      return (
        <pre key={i}>
          <code>{b.text}</code>
        </pre>
      );
    case "table":
      return (
        <div key={i} className="guide-table">
          <table>
            <thead>
              <tr>
                {b.header.map((cell, c) => (
                  <th key={c} style={{ textAlign: b.align[c] ?? undefined }}>
                    {inline(cell, cx)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {b.rows.map((row, r) => (
                <tr key={r}>
                  {row.map((cell, c) => (
                    <td key={c} style={{ textAlign: b.align[c] ?? undefined }}>
                      {inline(cell, cx)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "rule":
      return <hr key={i} />;
  }
}

/** The Markdown `text` of the guide page `page`, rendered as elements: no part of it becomes markup. */
export function GuideMarkdown({ text, page, images, onLink }: { text: string; page: string } & Pick<Context, "images" | "onLink">) {
  const blocks = useMemo(() => parseMarkdown(text), [text]);
  const cx: Context = { page, images, onLink };
  return <div className="guide-text">{blocks.map((b, i) => block(b, i, cx))}</div>;
}
