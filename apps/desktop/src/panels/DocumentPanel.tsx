// The document of the focused row of a MongoDB result, as a tree: the grid shows the
// top-level fields, nested documents and arrays arrive as JSON text (db_type `document`
// and `array`) and unfold here.
import { useEffect, useMemo, useState } from "react";
import { api, type ColumnInfo, toCommandError } from "../api";

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  /** Result row, `null` when none is focused. */
  row: number | null;
  /** The focused field, opened and highlighted in the tree. */
  field: string | null;
  /** Bumped when rows load. */
  version: number;
  onClose: () => void;
}

/** A cell as the value it holds: JSON columns parsed, numbers and booleans typed. */
function cellValue(column: ColumnInfo, text: string | null): Json {
  if (text === null) return null;
  switch (column.db_type) {
    case "document":
    case "array":
    case "mixed":
      try {
        return JSON.parse(text) as Json;
      } catch {
        return text;
      }
    case "bool":
      return text === "true";
    case "int":
    case "long":
    case "double":
      return Number.isNaN(Number(text)) ? text : Number(text);
    default:
      return text;
  }
}

/** Extended JSON wrappers shown as the shell writes them: `ObjectId("…")`, `ISODate("…")`. */
function wrapped(value: { [key: string]: Json }): string | null {
  const keys = Object.keys(value);
  if (keys.length !== 1) return null;
  const inner = value[keys[0]!];
  const text = typeof inner === "string" ? JSON.stringify(inner) : JSON.stringify(inner);
  switch (keys[0]) {
    case "$oid":
      return `ObjectId(${text})`;
    case "$date":
      return `ISODate(${typeof inner === "object" && inner && "$numberLong" in inner ? JSON.stringify(inner.$numberLong) : text})`;
    case "$numberDecimal":
      return `NumberDecimal(${text})`;
    case "$numberLong":
      return `NumberLong(${text})`;
    default:
      return null;
  }
}

function Leaf({ value }: { value: Json }) {
  if (value === null) return <span className="doc-null">null</span>;
  if (typeof value === "string") return <span className="doc-string">{JSON.stringify(value)}</span>;
  if (typeof value === "number" || typeof value === "boolean") return <span className="doc-number">{String(value)}</span>;
  return null;
}

function Node({ name, value, open, highlight }: { name: string; value: Json; open: boolean; highlight: boolean }) {
  const label = <span className={`doc-key${highlight ? " doc-focused" : ""}`}>{name}</span>;
  if (value === null || typeof value !== "object") {
    return (
      <div className="doc-line">
        {label}: <Leaf value={value} />
      </div>
    );
  }
  const shell = Array.isArray(value) ? null : wrapped(value);
  if (shell) {
    return (
      <div className="doc-line">
        {label}: <span className="doc-number">{shell}</span>
      </div>
    );
  }
  const entries: [string, Json][] = Array.isArray(value) ? value.map((v, i) => [String(i), v]) : Object.entries(value);
  const summary = Array.isArray(value) ? `[${value.length}]` : `{${entries.length}}`;
  return (
    <details className="doc-node" open={open}>
      <summary>
        {label} <span className="muted">{summary}</span>
      </summary>
      {entries.map(([k, v]) => (
        <Node key={k} name={k} value={v} open={false} highlight={false} />
      ))}
    </details>
  );
}

export function DocumentPanel({ jobId, columns, row, field, version, onClose }: Props) {
  const [values, setValues] = useState<(string | null)[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setValues(null);
    if (row === null || row < 0) return;
    let live = true;
    api.rowValues(jobId, row, 1).then(
      ([r]) => {
        if (!live) return;
        setValues(r ?? null);
        setError(null);
      },
      (e) => live && setError(toCommandError(e).message),
    );
    return () => {
      live = false;
    };
  }, [jobId, row, version]);

  const doc = useMemo(() => {
    if (!values) return null;
    const out: { [key: string]: Json } = {};
    columns.forEach((c, i) => {
      // Fields the document lacks are null cells; leave them out.
      if (values[i] !== null) out[c.name] = cellValue(c, values[i] ?? null);
    });
    return out;
  }, [values, columns]);

  return (
    <aside className="value-panel document-panel">
      <div className="value-bar">
        <strong className="small value-name">Document</strong>
        <span className="spacer" />
        {doc && (
          <button className="icon" title="Copy as JSON" onClick={() => void navigator.clipboard.writeText(JSON.stringify(doc, null, 2))}>
            ⧉
          </button>
        )}
        <button className="icon" title="Close (F7)" onClick={onClose}>
          ×
        </button>
      </div>
      {error ? (
        <div className="error-text small value-empty">{error}</div>
      ) : row === null ? (
        <div className="muted small value-empty">Select a row to see its document.</div>
      ) : !doc ? (
        <div className="muted small value-empty">Loading…</div>
      ) : (
        <div className="doc-tree">
          {Object.entries(doc).map(([k, v]) => (
            <Node key={k} name={k} value={v} open={k === field} highlight={k === field} />
          ))}
        </div>
      )}
    </aside>
  );
}
