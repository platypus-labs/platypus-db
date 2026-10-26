import { useId, useRef, useState } from "react";
import type { CellInput, FieldMeta } from "../../api";
import { LIST_MIN, LIST_SIZE, ListEditor } from "./ListEditor";
import { popupBox } from "./popupBox";
import { sameJson } from "./jsonChange";
import type { EditorKind } from "./meta";
import { t } from "../../i18n";

/** Where to go after committing: the next cell in a direction, or stay. */
export type Move = "down" | "up" | "right" | "left" | null;

interface Props {
  /** The cell's box within the grid. */
  rect: { left: number; top: number; width: number; height: number };
  /** Size of the grid, to keep the editor inside it. */
  bounds: { width: number; height: number };
  kind: EditorKind;
  /** The value to start from: `null` is NULL. */
  initial: string | null;
  /** Replaces the value, e.g. the key typed to start editing. */
  typed?: string;
  enumValues: string[];
  /** The column's type, shown by the array and composite editor. */
  type: string;
  /** Composite fields, for the composite editor. */
  fields: FieldMeta[];
  nullable: boolean;
  json: boolean;
  /** Number columns: an empty value means NULL. */
  numeric: boolean;
  /** Foreign keys: `[key, label]` rows of the table they point to, offered while typing. */
  options?: [string, string | null][];
  /** Binary and text columns: closes the editor and loads the cell from a file instead;
   *  `text` is what the editor held, to give back if no file is picked. */
  onLoadFile?: (text: string) => void;
  onCommit: (value: CellInput, move: Move) => void;
  onCancel: () => void;
}

const NULL_OPTION = "\u0000null";
const POPUP_SIZE = { width: 420, height: 240 };
const POPUP_MIN = { width: 260, height: 140 };
/** Width of the inline editor's "Load from file" button, at its right end. */
const FILE_BUTTON = 24;

/** The inline editor placed over a cell. */
export function CellEditor({
  rect,
  bounds,
  kind,
  initial,
  typed,
  enumValues,
  type,
  fields,
  nullable,
  json,
  numeric,
  options,
  onLoadFile,
  onCommit,
  onCancel,
}: Props) {
  const [text, setText] = useState(typed ?? initial ?? "");
  const listId = useId();
  const [problem, setProblem] = useState<string | null>(null);
  const done = useRef(false);

  /** Selects the value to replace it, or puts the caret after the key that started editing. */
  function onFocus(e: React.FocusEvent<HTMLInputElement | HTMLTextAreaElement>) {
    const el = e.currentTarget;
    if (el.type === "date") return;
    if (typed === undefined) el.select();
    else el.setSelectionRange(el.value.length, el.value.length);
  }

  function commit(move: Move, value = text) {
    if (done.current) return;
    if (json && value.trim()) {
      try {
        JSON.parse(value);
      } catch (e) {
        setProblem(t("Invalid JSON: {message}", { message: (e as Error).message }));
        return;
      }
    }
    done.current = true;
    // Formatting only changes how the JSON reads: keep the value as loaded if nothing else changed.
    if (json && initial !== null && typed === undefined && value !== initial && sameJson(initial, value)) {
      onCommit({ kind: "value", text: initial }, move);
      return;
    }
    const empty = value.trim() === "";
    onCommit(
      value === NULL_OPTION || ((numeric || kind === "lookup") && empty) ? { kind: "null" } : { kind: "value", text: value },
      move,
    );
  }

  function cancel() {
    done.current = true;
    onCancel();
  }

  /** The file's contents replace the value: what was typed is not committed, but handed
   *  over in case no file is picked. */
  function loadFile() {
    done.current = true;
    onLoadFile?.(text);
  }

  function onKeyDown(e: React.KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    } else if (e.key === "Tab") {
      e.preventDefault();
      commit(e.shiftKey ? "left" : "right");
    } else if (e.key === "Enter" && (kind !== "multiline" || e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      commit(kind === "multiline" ? null : e.shiftKey ? "up" : "down");
    }
  }

  if (kind === "array" || kind === "composite") {
    return (
      <ListEditor
        box={popupBox(rect, bounds, LIST_SIZE, LIST_MIN)}
        composite={kind === "composite"}
        type={type}
        fields={fields}
        initial={initial}
        typed={typed}
        nullable={nullable}
        onCommit={onCommit}
        onCancel={onCancel}
      />
    );
  }

  if (kind === "multiline") {
    return (
      <div
        className="cell-popup"
        style={popupBox(rect, bounds, POPUP_SIZE, POPUP_MIN)}
        onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && commit(null)}
      >
        <textarea
          autoFocus
          onFocus={onFocus}
          className="cell-popup-text"
          value={text}
          spellCheck={false}
          onChange={(e) => {
            setText(e.target.value);
            setProblem(null);
          }}
          onKeyDown={onKeyDown}
        />
        {problem && <div className="error-text small">{problem}</div>}
        <div className="cell-popup-actions">
          {json && (
            <button
              type="button"
              onClick={() => {
                try {
                  setText(JSON.stringify(JSON.parse(text), null, 2));
                } catch (e) {
                  setProblem(t("Invalid JSON: {message}", { message: (e as Error).message }));
                }
              }}
            >
              {t("Format JSON")}
            </button>
          )}
          {onLoadFile && (
            <button type="button" onClick={loadFile}>
              {t("Load from file…")}
            </button>
          )}
          <span className="spacer" />
          <span className="muted small">{t("Ctrl+Enter to apply")}</span>
          <span className="cell-popup-end">
            <button type="button" onClick={cancel}>
              {t("Cancel")}
            </button>
            <button type="button" className="primary" onClick={() => commit(null)}>
              {t("Apply")}
            </button>
          </span>
        </div>
      </div>
    );
  }

  const style = { left: rect.left, top: rect.top, width: Math.max(rect.width, 120), height: rect.height };
  const onBlur = () => commit(null);
  if (kind === "bool" || kind === "enum") {
    const options = kind === "bool" ? ["true", "false"] : enumValues;
    const current = initial === null && typed === undefined ? NULL_OPTION : text;
    return (
      <select
        autoFocus
        className="cell-editor"
        style={style}
        defaultValue={current}
        onChange={(e) => commit(null, e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={onBlur}
      >
        {!options.includes(current) && current !== NULL_OPTION && <option value={current}>{current}</option>}
        {options.map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
        {nullable && <option value={NULL_OPTION}>NULL</option>}
      </select>
    );
  }
  return (
    <>
      <input
        autoFocus
        onFocus={onFocus}
        className="cell-editor"
        style={onLoadFile ? { ...style, paddingRight: FILE_BUTTON + 4 } : style}
        type={kind === "date" ? "date" : "text"}
        value={text}
        spellCheck={false}
        list={kind === "lookup" ? listId : undefined}
        onChange={(e) => {
          setText(e.target.value);
          // Picking a row from the list is an input without an input type: apply it.
          if (kind === "lookup" && !(e.nativeEvent as InputEvent).inputType) commit(null, e.target.value);
        }}
        onKeyDown={onKeyDown}
        onBlur={onBlur}
      />
      {onLoadFile && (
        <button
          type="button"
          className="cell-editor-file"
          style={{ left: style.left + style.width - FILE_BUTTON, top: style.top, width: FILE_BUTTON, height: style.height }}
          title={t("Load from file…")}
          aria-label={t("Load from file…")}
          // Keeps the focus in the input, whose blur would commit it.
          onMouseDown={(e) => e.preventDefault()}
          onClick={loadFile}
        >
          …
        </button>
      )}
      {kind === "lookup" && (
        <datalist id={listId}>
          {options?.map(([key, label]) => (
            <option key={key} value={key}>
              {label ?? ""}
            </option>
          ))}
        </datalist>
      )}
    </>
  );
}
