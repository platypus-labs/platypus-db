// The document of the focused row of a MongoDB, Redis or Elasticsearch result, as a tree: the
// grid shows the top-level fields, nested documents and arrays arrive as JSON text (db_type `document`,
// `array`, `mixed`, Redis's `json`) and unfold here.
import { useEffect, useMemo, useState } from "react";
import { api, type ColumnInfo, toCommandError } from "../api";
import { t } from "../i18n";
import { MASK, useMask } from "../guards/mask";
import { JsonNumber, type JsonValue as Json, jsonNumber, parseJson, stringifyJson } from "../jsonText";
import { EmptyState, LoadingState } from "../ui/State";

/** An object of the document: not a leaf, not an array. */
const isObject = (value: Json): value is { [key: string]: Json } =>
  value !== null && typeof value === "object" && !Array.isArray(value) && !(value instanceof JsonNumber);

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

/** A cell as the value it holds: JSON columns parsed, numbers and booleans typed. Numbers keep
 *  their digits: a `long` above 2^53 is not rounded. */
function cellValue(column: ColumnInfo, text: string | null): Json {
  if (text === null) return null;
  switch (column.db_type) {
    case "document":
    case "array":
    case "mixed":
    case "json":
      try {
        return parseJson(text);
      } catch {
        return text;
      }
    case "bool":
      return text === "true";
    // Elasticsearch does not coerce `_source`: a value that is no boolean stays text.
    case "boolean":
      return text === "true" ? true : text === "false" ? false : text;
    case "int":
    case "long":
    case "double":
    case "integer":
    case "ttl":
    case "short":
    case "byte":
    case "float":
    case "half_float":
    case "scaled_float":
    case "unsigned_long":
      return jsonNumber(text) ?? text;
    default:
      return text;
  }
}

/** Extended JSON wrappers shown as the shell writes them: `ObjectId("…")`, `ISODate("…")`. */
function wrapped(value: { [key: string]: Json }): string | null {
  const keys = Object.keys(value);
  if (keys.length !== 1) return null;
  const inner = value[keys[0]!]!;
  const text = stringifyJson(inner);
  switch (keys[0]) {
    case "$oid":
      return `ObjectId(${text})`;
    case "$date":
      return `ISODate(${isObject(inner) && inner.$numberLong !== undefined ? stringifyJson(inner.$numberLong) : text})`;
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
  if (value instanceof JsonNumber || typeof value === "boolean") return <span className="doc-number">{String(value)}</span>;
  return null;
}

/** Whether a field's value shows masked, by its name; `null` while masking is off. */
type Hidden = ((name: string) => boolean) | null;

function Node({ name, value, open, highlight, hidden }: { name: string; value: Json; open: boolean; highlight: boolean; hidden: Hidden }) {
  const label = <span className={`doc-key${highlight ? " doc-focused" : ""}`}>{name}</span>;
  // A masked field shows dots in place of its value, nested fields included.
  if (value !== null && hidden?.(name)) {
    return (
      <div className="doc-line">
        {label}: <span className="doc-string">{MASK}</span>
      </div>
    );
  }
  if (value === null || typeof value !== "object" || value instanceof JsonNumber) {
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
        <Node key={k} name={k} value={v} open={false} highlight={false} hidden={hidden} />
      ))}
    </details>
  );
}

export function DocumentPanel({ jobId, columns, row, field, version, onClose }: Props) {
  const [values, setValues] = useState<(string | null)[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mask = useMask();
  const hidden: Hidden = mask && ((name) => mask({ name, db_type: "" }));

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
        <strong className="small value-name">{t("Document")}</strong>
        <span className="spacer" />
        {doc && (
          <button className="icon" title={t("Copy as JSON")} aria-label={t("Copy as JSON")} onClick={() => void navigator.clipboard.writeText(stringifyJson(doc))}>
            ⧉
          </button>
        )}
        <button className="icon" title={t("Close (F7)")} aria-label={t("Close (F7)")} onClick={onClose}>
          ×
        </button>
      </div>
      {error ? (
        <div className="error-text small value-empty">{error}</div>
      ) : row === null ? (
        <EmptyState compact className="value-empty" title={t("Select a row to see its document.")} />
      ) : !doc ? (
        <LoadingState compact className="value-empty" title={t("Loading…")} />
      ) : (
        <div className="doc-tree">
          {Object.entries(doc).map(([k, v]) => (
            <Node key={k} name={k} value={v} open={k === field} highlight={k === field} hidden={hidden} />
          ))}
        </div>
      )}
    </aside>
  );
}
