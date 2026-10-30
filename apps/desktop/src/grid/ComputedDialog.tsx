import { useState } from "react";
import type { ColumnInfo } from "../api";
import { t } from "../i18n";
import { type ComputedColumn, compile, ExpressionError, FUNCTION_NAMES } from "./computed";
import { EmptyState } from "../ui/State";
import { Icon } from "../ui/Icon";

interface Props {
  /** The result's columns, which expressions can use. */
  columns: ColumnInfo[];
  computed: ComputedColumn[];
  /** Kept for the table (data viewer) rather than for this result only. */
  saved: boolean;
  onChange: (computed: ComputedColumn[]) => void;
  onClose: () => void;
}

/** What is wrong with an expression, for the user. */
function problem(e: unknown): string {
  if (!(e instanceof ExpressionError)) return t("The expression is not valid.");
  switch (e.reason) {
    case "column":
      return t("No column named {name}.", { name: e.near });
    case "function":
      return t("No function named {name}.", { name: e.near });
    case "arguments":
      return t("Wrong number of values for {name}.", { name: e.near });
    default:
      return e.near ? t("The expression is not valid near: {text}", { text: e.near }) : t("The expression is incomplete.");
  }
}

/** Adds, changes and removes a grid's computed columns: a name and an expression on the row. */
export function ComputedDialog({ columns, computed, saved, onChange, onClose }: Props) {
  const [editing, setEditing] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [expression, setExpression] = useState("");

  const trimmed = name.trim();
  const taken = (n: string) =>
    columns.some((c) => c.name === n) || computed.some((c, i) => i !== editing && c.name === n);
  let error: string | null = null;
  if (expression.trim()) {
    try {
      compile(expression, columns);
    } catch (e) {
      error = problem(e);
    }
  }
  if (!error && trimmed && taken(trimmed)) error = t("A column named {name} already exists.", { name: trimmed });
  const ready = trimmed !== "" && expression.trim() !== "" && error === null;

  const reset = () => {
    setEditing(null);
    setName("");
    setExpression("");
  };
  const save = () => {
    if (!ready) return;
    const column = { name: trimmed, expression: expression.trim() };
    onChange(editing === null ? [...computed, column] : computed.map((c, i) => (i === editing ? column : c)));
    reset();
  };
  const edit = (i: number) => {
    setEditing(i);
    setName(computed[i]!.name);
    setExpression(computed[i]!.expression);
  };
  const remove = (i: number) => {
    onChange(computed.filter((_, j) => j !== i));
    if (i === editing) reset();
    // The one being edited moves up when one before it goes.
    else if (editing !== null && i < editing) setEditing(editing - 1);
  };
  const enter = (e: React.KeyboardEvent) => e.key === "Enter" && save();

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal formatting-dialog computed-dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Computed columns")}</h2>
        <p className="muted small">
          {t(
            saved
              ? "Columns worked out from the others of the same row, only on screen. They are kept for this table."
              : "Columns worked out from the others of the same row, only on screen. They last while this result is open.",
          )}
        </p>
        <div className="formatting-rules" data-count={computed.length}>
          {computed.length === 0 && <EmptyState compact className="formatting-empty" title={t("No computed columns yet.")} />}
          {computed.map((c, i) => (
            <div key={c.name} className={`formatting-rule${i === editing ? " selected" : ""}`} data-computed={c.name}>
              <button type="button" className="link formatting-text" onClick={() => edit(i)} title={t("Edit")}>
                {c.name} = {c.expression}
              </button>
              <span className="spacer" />
              <button type="button" onClick={() => remove(i)} title={t("Remove")} aria-label={t("Remove")}>
                <Icon name="close" />
              </button>
            </div>
          ))}
        </div>

        <h3>{t(editing === null ? "New computed column" : "Edit computed column")}</h3>
        <div className="computed-form">
          <label className="field">
            <span>{t("Name")}</span>
            <input data-field="name" value={name} onChange={(e) => setName(e.target.value)} onKeyDown={enter} autoFocus />
          </label>
          <label className="field">
            <span>{t("Expression")}</span>
            <input
              data-field="expression"
              className="mono"
              spellCheck={false}
              placeholder="price * qty"
              value={expression}
              onChange={(e) => setExpression(e.target.value)}
              onKeyDown={enter}
            />
          </label>
        </div>
        <p className={`small ${error ? "error-text" : "muted"}`} data-field="problem">
          {error ?? t("Columns by name, quoted with \"…\" when they have spaces; + - * / %, || to join text, comparisons, AND, OR, NOT; functions: {list}.", { list: FUNCTION_NAMES.join(", ") })}
        </p>
        <div className="actions">
          {editing !== null && (
            <button type="button" onClick={reset}>
              {t("New computed column")}
            </button>
          )}
          <span className="spacer" />
          <button type="button" data-action="save-computed" onClick={save} disabled={!ready}>
            {t(editing === null ? "Add column" : "Update column")}
          </button>
          <button type="button" className="primary" onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
