import { useEffect, useRef, useState } from "react";
import { api, type CellInput, type FieldMeta, toCommandError } from "../../api";
import type { Move } from "./CellEditor";

interface Props {
  /** Where the popup goes, inside the grid. */
  left: number;
  top: number;
  /** A composite value (one row per field) rather than an array (one row per element). */
  composite: boolean;
  /** The column's type, e.g. `integer[]`. */
  type: string;
  /** The composite type's fields, or those of the array's composite elements. */
  fields: FieldMeta[];
  /** The value to start from: `null` is NULL. */
  initial: string | null;
  /** The key that started editing: the value is then typed as text. */
  typed?: string;
  nullable: boolean;
  onCommit: (value: CellInput, move: Move) => void;
  onCancel: () => void;
}

export const LIST_W = 440;
export const LIST_H = 300;

/** The editor of a PostgreSQL array or composite value: its elements or fields one per row,
 *  each with its own NULL, or the whole literal as text. */
export function ListEditor({ left, top, composite, type, fields, initial, typed, nullable, onCommit, onCancel }: Props) {
  /** `null` while the value is being split. */
  const [items, setItems] = useState<(string | null)[] | null>(null);
  const [asText, setAsText] = useState(typed !== undefined);
  const [text, setText] = useState(typed ?? initial ?? "");
  const [problem, setProblem] = useState<string | null>(null);
  const done = useRef(false);
  const element = composite ? null : type.replace(/\[\]$/, "");

  useEffect(() => {
    if (typed !== undefined) return;
    if (initial === null) {
      setItems(composite ? fields.map(() => null) : []);
      return;
    }
    let live = true;
    api.splitLiteral(composite, initial).then(
      (split) => {
        if (!live) return;
        if (split) setItems(split);
        else setAsText(true); // nested arrays and the like: as text
      },
      () => live && setAsText(true),
    );
    return () => {
      live = false;
    };
  }, []);

  async function commit(value?: CellInput) {
    if (done.current) return;
    try {
      const input: CellInput = value ?? {
        kind: "value",
        text: asText || !items ? text : await api.joinLiteral(composite, items),
      };
      done.current = true;
      onCommit(input, null);
    } catch (e) {
      setProblem(toCommandError(e).message);
    }
  }

  function cancel() {
    done.current = true;
    onCancel();
  }

  async function toggleText() {
    setProblem(null);
    if (!asText) {
      if (items) setText(await api.joinLiteral(composite, items));
      setAsText(true);
      return;
    }
    const split = await api.splitLiteral(composite, text);
    if (!split) {
      setProblem(composite ? "Not a composite value: (a,b,…)" : "Not a one-dimensional array: {a,b,…}");
      return;
    }
    setItems(split);
    setAsText(false);
  }

  function onKeyDown(e: React.KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void commit();
    }
  }

  const set = (i: number, value: string | null) => setItems((all) => all && all.map((v, j) => (j === i ? value : v)));

  return (
    <div
      className="cell-popup list-editor"
      style={{ left, top, width: LIST_W, height: LIST_H }}
      onKeyDown={onKeyDown}
      onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && void commit()}
    >
      <div className="list-editor-head muted small">
        <span>{type}</span>
        {!asText && items && !composite && <span>{items.length === 1 ? "1 element" : `${items.length} elements`}</span>}
      </div>
      {asText ? (
        <textarea
          autoFocus
          className="cell-popup-text"
          value={text}
          spellCheck={false}
          onChange={(e) => {
            setText(e.target.value);
            setProblem(null);
          }}
        />
      ) : (
        <div className="list-editor-rows" tabIndex={-1}>
          {items?.map((value, i) => (
            <div className="list-editor-row" key={i}>
              <span className="list-editor-label" title={fields[i]?.data_type ?? element ?? undefined}>
                {composite ? (fields[i]?.name ?? `#${i + 1}`) : `[${i + 1}]`}
              </span>
              <input
                autoFocus={i === 0}
                value={value ?? ""}
                placeholder={value === null ? "NULL" : undefined}
                spellCheck={false}
                onChange={(e) => set(i, e.target.value)}
              />
              <label className="check small" title="NULL">
                <input type="checkbox" checked={value === null} onChange={(e) => set(i, e.target.checked ? null : "")} />
                NULL
              </label>
              {!composite && (
                <button
                  type="button"
                  className="icon"
                  title="Remove the element"
                  onClick={() => setItems((all) => all && all.filter((_, j) => j !== i))}
                >
                  ×
                </button>
              )}
            </div>
          ))}
          {!composite && items && (
            <button type="button" className="list-editor-add" onClick={() => setItems((all) => all && [...all, ""])}>
              + Element
            </button>
          )}
        </div>
      )}
      {problem && <div className="error-text small">{problem}</div>}
      <div className="cell-popup-actions">
        <button type="button" onClick={() => void toggleText()}>
          {asText ? "Edit as list" : "Edit as text"}
        </button>
        {nullable && (
          <button type="button" onClick={() => void commit({ kind: "null" })}>
            Set NULL
          </button>
        )}
        <span className="spacer" />
        <span className="muted small">Ctrl+Enter to apply</span>
        <button type="button" onClick={cancel}>
          Cancel
        </button>
        <button type="button" className="primary" onClick={() => void commit()}>
          Apply
        </button>
      </div>
    </div>
  );
}
