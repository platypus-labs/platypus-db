import { useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, type ColumnInfo, toCommandError } from "../api";
import { formatCount, t, plural } from "../i18n";
import { JSON_TYPE, formatJson, isJsonDocument } from "../jsonText";

/** The focused cell of a result. */
export interface ValueTarget {
  jobId: number;
  /** Row of the result (not counting new rows of the editor). */
  row: number;
  col: number;
  column: ColumnInfo;
  /** SQL NULL, as the grid shows it. */
  isNull: boolean;
  /** JSON in a text column is shown as written, not indented (the connection's choice). */
  plainTextJson?: boolean;
}

const jsonColumn = (column: ColumnInfo) => JSON_TYPE.test(column.db_type);

type Mode = "text" | "json" | "xml" | "hex" | "image";

/** Hex dumps stop here; the full value can still be saved. */
const HEX_LIMIT = 64 * 1024;
/** Characters of text shown at first, and added by each "Show more": one element holding
 *  megabytes of text takes the WebView gigabytes and seconds to lay out. */
const TEXT_PAGE = 256 * 1024;
/** Longer values are not parsed as JSON or XML to be indented. */
const FORMAT_LIMIT = 1024 * 1024;

const IMAGE_TYPES: [string, number[]][] = [
  ["image/png", [0x89, 0x50, 0x4e, 0x47]],
  ["image/jpeg", [0xff, 0xd8, 0xff]],
  ["image/gif", [0x47, 0x49, 0x46, 0x38]],
  ["image/webp", [0x52, 0x49, 0x46, 0x46]],
  ["image/bmp", [0x42, 0x4d]],
];

function imageType(bytes: Uint8Array): string | null {
  for (const [type, magic] of IMAGE_TYPES) {
    if (magic.every((b, i) => bytes[i] === b)) return type;
  }
  return null;
}

function hexDump(bytes: Uint8Array): string {
  const lines: string[] = [];
  const end = Math.min(bytes.length, HEX_LIMIT);
  for (let i = 0; i < end; i += 16) {
    const chunk = bytes.subarray(i, Math.min(i + 16, end));
    const hex = Array.from(chunk, (b) => b.toString(16).padStart(2, "0")).join(" ");
    const ascii = Array.from(chunk, (b) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".")).join("");
    lines.push(`${i.toString(16).padStart(8, "0")}  ${hex.padEnd(47)}  ${ascii}`);
  }
  if (bytes.length > end) lines.push(`… ${t("{n} more bytes", { n: formatCount(bytes.length - end) })}`);
  return lines.join("\n");
}

/** Indents XML by element depth; returns `null` when the text does not parse. */
function prettyXml(text: string): string | null {
  const doc = new DOMParser().parseFromString(text, "application/xml");
  if (doc.querySelector("parsererror")) return null;
  const out: string[] = [];
  const walk = (node: Node, depth: number) => {
    const pad = "  ".repeat(depth);
    if (node.nodeType === Node.TEXT_NODE) {
      const t = node.textContent?.trim();
      if (t) out.push(pad + t);
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const el = node as Element;
    const attrs = Array.from(el.attributes, (a) => ` ${a.name}="${a.value}"`).join("");
    const children = Array.from(el.childNodes).filter((c) => c.nodeType !== Node.TEXT_NODE || c.textContent?.trim());
    if (children.length === 0) out.push(`${pad}<${el.tagName}${attrs}/>`);
    else if (children.length === 1 && children[0]!.nodeType === Node.TEXT_NODE) {
      out.push(`${pad}<${el.tagName}${attrs}>${children[0]!.textContent?.trim()}</${el.tagName}>`);
    } else {
      out.push(`${pad}<${el.tagName}${attrs}>`);
      children.forEach((c) => walk(c, depth + 1));
      out.push(`${pad}</${el.tagName}>`);
    }
  };
  walk(doc.documentElement, 0);
  return out.join("\n");
}

/** An object or array, indented; `null` for anything else. A plain number is valid JSON
 *  too, but shows as text. Numbers keep their digits (no rounding above 2^53). */
function prettyJson(text: string): string | null {
  return isJsonDocument(text) ? formatJson(text) : null;
}

/** The focused cell's whole value: text, JSON or XML formatted, hex, or an image. */
export function ValuePanel({ target, onClose }: { target: ValueTarget | null; onClose: () => void }) {
  const [bytes, setBytes] = useState<Uint8Array<ArrayBuffer> | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode | null>(null);
  /** Characters of text shown, a multiple of `TEXT_PAGE`. */
  const [limit, setLimit] = useState(TEXT_PAGE);
  const binary = target?.column.binary ?? false;

  useEffect(() => {
    setBytes(null);
    setError(null);
    setMode(null);
    setLimit(TEXT_PAGE);
    if (!target) return;
    let live = true;
    api.cellBytes(target.jobId, target.row, target.col).then(
      (b) => live && setBytes(b),
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [target?.jobId, target?.row, target?.col]);

  const text = useMemo(() => (bytes && !binary ? new TextDecoder().decode(bytes) : ""), [bytes, binary]);
  const image = bytes && binary ? imageType(bytes) : null;
  const url = useMemo(() => (bytes && image ? URL.createObjectURL(new Blob([bytes], { type: image })) : null), [bytes, image]);
  useEffect(() => () => void (url && URL.revokeObjectURL(url)), [url]);

  const formatted = useMemo(() => {
    const plain = binary || text.length > FORMAT_LIMIT;
    const json = plain ? null : prettyJson(text);
    const xml = plain || json !== null || !text.trimStart().startsWith("<") ? null : prettyXml(text);
    return { json, xml };
  }, [text, binary]);
  // JSON a text column holds opens indented, unless the connection keeps it as written: the
  // JSON view is still a click away.
  const openJson = formatted.json !== null && (!target?.plainTextJson || jsonColumn(target.column));
  const auto: Mode = image ? "image" : binary ? "hex" : openJson ? "json" : formatted.xml !== null ? "xml" : "text";
  const shown = mode ?? auto;
  const modes: Mode[] = binary
    ? image
      ? ["image", "hex"]
      : ["hex"]
    : ["text", ...(formatted.json !== null ? (["json"] as const) : []), ...(formatted.xml !== null ? (["xml"] as const) : []), "hex"];

  async function saveValue() {
    if (!target) return;
    const path = await save({ defaultPath: `${target.column.name}.${image?.split("/")[1] ?? (binary ? "bin" : "txt")}` });
    if (!path) return;
    try {
      await api.saveCell(target.jobId, target.row, target.col, path);
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const body = (() => {
    if (!target) return <div className="muted small value-empty">{t("Select a cell to see its value.")}</div>;
    if (error) return <div className="error-text small value-empty">{error}</div>;
    if (!bytes) return <div className="muted small value-empty">{t("Loading…")}</div>;
    if (target.isNull) return <div className="muted small value-empty">NULL</div>;
    if (bytes.length === 0) return <div className="muted small value-empty">{t("Empty")}</div>;
    switch (shown) {
      case "image":
        return url && <img className="value-image" src={url} alt={target.column.name} />;
      case "hex":
        return <pre className="value-text">{hexDump(bytes)}</pre>;
      case "json":
        return <pre className="value-text">{formatted.json}</pre>;
      case "xml":
        return <pre className="value-text">{formatted.xml}</pre>;
      default:
        return (
          <div className="value-text">
            <pre>{text.length > limit ? text.slice(0, limit) : text}</pre>
            {text.length > limit && (
              <div className="muted small value-more">
                {t("Showing {shown} of {total} characters.", {
                  shown: formatCount(limit),
                  total: formatCount(text.length),
                })}{" "}
                <button className="link" onClick={() => setLimit((n) => n + TEXT_PAGE)}>
                  {t("Show more")}
                </button>
              </div>
            )}
          </div>
        );
    }
  })();

  return (
    <aside className="value-panel">
      <div className="value-bar">
        <strong className="small value-name" title={target?.column.db_type}>
          {target?.column.name ?? t("Value")}
        </strong>
        {bytes && bytes.length > 0 && (
          <select value={shown} onChange={(e) => setMode(e.target.value as Mode)} title={t("Show as")} aria-label={t("Show as")}>
            {modes.map((m) => (
              <option key={m} value={m}>
                {m === "json" ? "JSON" : m === "xml" ? "XML" : m === "hex" ? "Hex" : t(m === "image" ? "Image" : "Text")}
              </option>
            ))}
          </select>
        )}
        <span className="spacer" />
        {bytes && bytes.length > 0 && !binary && (
          <button className="icon" title={t("Copy")} aria-label={t("Copy")} onClick={() => void navigator.clipboard.writeText(shown === "text" ? text : (formatted[shown as "json" | "xml"] ?? text))}>
            ⧉
          </button>
        )}
        <button className="icon" title={t("Save to file…")} aria-label={t("Save to file…")} onClick={() => void saveValue()} disabled={!bytes}>
          ⤓
        </button>
        <button className="icon" title={t("Close (F7)")} aria-label={t("Close (F7)")} onClick={onClose}>
          ×
        </button>
      </div>
      {body}
      {bytes && <div className="muted small value-size">{plural(bytes.length, "{n} byte", "{n} bytes")}</div>}
    </aside>
  );
}
