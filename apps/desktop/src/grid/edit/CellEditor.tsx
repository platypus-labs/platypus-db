import { useRef, useState } from "react";
import type { CellInput } from "../../api";
import type { EditorKind } from "./meta";

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
  nullable: boolean;
  json: boolean;
  /** Number columns: an empty value means NULL. */
  numeric: boolean;
  onCommit: (value: CellInput, move: Move) => void;
  onCancel: () => void;
}

const NULL_OPTION = "\u0000null";
const POPUP_W = 420;
const POPUP_H = 240;

/** The inline editor placed over a cell. */
export function CellEditor({
  rect,
  bounds,
  kind,
  initial,
  typed,
  enumValues,
  nullable,
  json,
  numeric,
  onCommit,
  onCancel,
}: Props) {
  const [text, setText] = useState(typed ?? initial ?? "");
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
        setProblem(`Invalid JSON: ${(e as Error).message}`);
        return;
      }
    }
    done.current = true;
    const empty = value.trim() === "";
    onCommit(
      value === NULL_OPTION || (numeric && empty) ? { kind: "null" } : { kind: "value", text: value },
      move,
    );
  }

  function cancel() {
    done.current = true;
    onCancel();
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

  if (kind === "multiline") {
    const left = Math.max(0, Math.min(rect.left, bounds.width - POPUP_W - 4));
    const top = Math.max(0, Math.min(rect.top, bounds.height - POPUP_H - 4));
    return (
      <div
        className="cell-popup"
        style={{ left, top, width: POPUP_W, height: POPUP_H }}
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
                  setProblem(`Invalid JSON: ${(e as Error).message}`);
                }
              }}
            >
              Format
            </button>
          )}
          <span className="spacer" />
          <span className="muted small">Ctrl+Enter to apply</span>
          <button type="button" onClick={cancel}>
            Cancel
          </button>
          <button type="button" className="primary" onClick={() => commit(null)}>
            Apply
          </button>
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
    <input
      autoFocus
      onFocus={onFocus}
      className="cell-editor"
      style={style}
      type={kind === "date" ? "date" : "text"}
      value={text}
      spellCheck={false}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={onKeyDown}
      onBlur={onBlur}
    />
  );
}
