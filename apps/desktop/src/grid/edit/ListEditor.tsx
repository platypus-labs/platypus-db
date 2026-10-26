import { useEffect, useRef, useState } from "react";
import { api, type CellInput, type FieldMeta, toCommandError } from "../../api";
import type { Move } from "./CellEditor";
import type { Box } from "./popupBox";
import { t, plural } from "../../i18n";

interface Props {
  /** Where the popup goes, inside the grid, and its size. */
  box: Box;
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

/** The popup's size when the grid has room for it, and the least it shrinks to. */
export const LIST_SIZE = { width: 440, height: 300 };
export const LIST_MIN = { width: 280, height: 160 };

/** The editor of a PostgreSQL array or composite value: its elements or fields one per row,
 *  each with its own NULL, or the whole literal as text. A header names the type, the rows
 *  scroll, and the actions stay at the bottom. */
export function ListEditor({ box, composite, type, fields, initial, typed, nullable, onCommit, onCancel }: Props) {
  /** `null` while the value is being split. */
  const [items, setItems] = useState<(string | null)[] | null>(null);
  const [asText, setAsText] = useState(typed !== undefined);
  const [text, setText] = useState(typed ?? initial ?? "");
  const [problem, setProblem] = useState<string | null>(null);
  const done = useRef(false);
  const rows = useRef<HTMLDivElement>(null);
  /** An element was just added: its field takes the focus. */
  const added = useRef(false);
  const element = composite ? null : type.replace(/\[\]$/, "");

  useEffect(() => {
    if (typed !== undefined) return;
    if (initial === null) {
      setItems(composite ? fields.map(() => null) : []);
      return;
    }
    let live = true;
    api.splitLiteral(composite, type, initial).then(
      (split) => {
        if (!live) return;
        if (split) setItems(split);
        else setAsText(true); // nested arrays, explicit bounds and the like: as text
      },
      () => live && setAsText(true),
    );
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    if (!added.current) return;
    added.current = false;
    const inputs = rows.current?.querySelectorAll<HTMLInputElement>("input:not([type=checkbox])");
    inputs?.[inputs.length - 1]?.focus();
  }, [items?.length]);

  async function commit(value?: CellInput) {
    if (done.current) return;
    try {
      const input: CellInput = value ?? {
        kind: "value",
        text: asText || !items ? text : await api.joinLiteral(composite, type, items),
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
    try {
      if (!asText) {
        if (items) setText(await api.joinLiteral(composite, type, items));
        setAsText(true);
        return;
      }
      const split = await api.splitLiteral(composite, type, text);
      if (!split) {
        setProblem(t(composite ? "Not a composite value: (a,b,…)" : "Not a one-dimensional array: {a,b,…}"));
        return;
      }
      setItems(split);
      setAsText(false);
    } catch (e) {
      setProblem(toCommandError(e).message);
    }
  }

  function onKeyDown(e: React.KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey || isTextInput(e.target))) {
      e.preventDefault();
      void commit();
    }
  }

  const edit = (change: (all: (string | null)[]) => (string | null)[]) => setItems((all) => all && change(all));
  const set = (i: number, value: string | null) => edit((all) => all.map((v, j) => (j === i ? value : v)));
  /** The focus moves to the rows before the element's row goes, so that the popup keeps it
   *  and does not take the removal for a click elsewhere. */
  const remove = (i: number) => {
    rows.current?.focus();
    edit((all) => all.filter((_, j) => j !== i));
  };

  return (
    <div
      className="cell-popup list-editor"
      // Focusable, so a click on its header or padding keeps the focus inside it.
      tabIndex={-1}
      style={box}
      onKeyDown={onKeyDown}
      onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && void commit()}
    >
      <div className="cell-popup-head list-editor-head muted small">
        <span className="list-editor-type" title={type}>
          {type}
          {!asText && items && !composite && ` · ${plural(items.length, "{n} element", "{n} elements")}`}
        </span>
        <span>{t("Ctrl+Enter to apply")}</span>
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
        <div className="list-editor-rows" ref={rows} tabIndex={-1}>
          {items?.map((value, i) => (
            <div className="list-editor-row" key={i}>
              <span className="list-editor-label" title={(composite ? fields[i]?.data_type : element) ?? undefined}>
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
                  title={t("Remove the element")}
                  aria-label={t("Remove the element")}
                  onClick={() => remove(i)}
                >
                  ×
                </button>
              )}
            </div>
          ))}
          {!composite && items && (
            <button
              type="button"
              className="list-editor-add"
              onClick={() => {
                added.current = true;
                edit((all) => [...all, ""]);
              }}
            >
              {t("+ Element")}
            </button>
          )}
        </div>
      )}
      {problem && <div className="error-text small">{problem}</div>}
      <div className="cell-popup-actions">
        <button type="button" onClick={() => void toggleText()}>
          {t(asText ? "Edit as list" : "Edit as text")}
        </button>
        {nullable && (
          <button type="button" onClick={() => void commit({ kind: "null" })}>
            {t("Set NULL")}
          </button>
        )}
        <span className="spacer" />
        <span className="cell-popup-end">
          <button type="button" onClick={cancel}>
            {t("Cancel")}
          </button>
          <button type="button" className="primary" onClick={() => void commit()}>
            {t("Apply")}
          </button>
        </span>
      </div>
    </div>
  );
}

/** Enter applies from an element's field, as in the other cell editors; the text area keeps
 *  it for new lines. */
function isTextInput(target: EventTarget): boolean {
  return target instanceof HTMLInputElement && target.type !== "checkbox";
}
