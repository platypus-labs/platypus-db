import { useState } from "react";
import type { ColumnInfo } from "../api";
import { msg, t } from "../i18n";
import {
  COLOR_NAMES,
  type ColorName,
  type Condition,
  type ConditionKind,
  type FormatRule,
  colorOf,
  complete,
  conditionOf,
  moveRule,
  scaleColor,
} from "./formatting";

interface Props {
  columns: ColumnInfo[];
  rules: FormatRule[];
  /** The column the new rule starts on: the focused one. */
  column: string | null;
  /** Rules of a table are kept; those of a query only while its result is open. */
  saved: boolean;
  onChange: (rules: FormatRule[]) => void;
  onClose: () => void;
}

const KINDS: { kind: ConditionKind; label: string; numeric?: boolean }[] = [
  { kind: "equals", label: msg("Equal to") },
  { kind: "not_equals", label: msg("Not equal to") },
  { kind: "contains", label: msg("Contains") },
  { kind: "between", label: msg("Between") },
  { kind: "null", label: msg("Is NULL") },
  { kind: "not_null", label: msg("Is not NULL") },
  { kind: "scale", label: msg("Color scale"), numeric: true },
];

const COLOR_LABELS: Record<ColorName, string> = {
  red: msg("Red"),
  orange: msg("Orange"),
  yellow: msg("Yellow"),
  green: msg("Green"),
  teal: msg("Teal"),
  blue: msg("Blue"),
  purple: msg("Purple"),
  gray: msg("Gray"),
};

/** A rule as one line: `amount between 10 and 20`. */
function describe(rule: FormatRule): string {
  const { column, condition: c } = rule;
  switch (c.kind) {
    case "equals":
      return t("{column} = {value}", { column, value: c.value });
    case "not_equals":
      return t("{column} ≠ {value}", { column, value: c.value });
    case "contains":
      return t("{column} contains {value}", { column, value: c.value });
    case "between":
      if (c.min.trim() === "") return t("{column} ≤ {value}", { column, value: c.max });
      if (c.max.trim() === "") return t("{column} ≥ {value}", { column, value: c.min });
      return t("{column} between {min} and {max}", { column, min: c.min, max: c.max });
    case "null":
      return t("{column} is NULL", { column });
    case "not_null":
      return t("{column} is not NULL", { column });
    case "scale":
      return t("{column}: color scale", { column });
  }
}

const newRule = (column: string): FormatRule => ({
  id: crypto.randomUUID(),
  column,
  condition: { kind: "equals", value: "" },
  color: "red",
  row: false,
});

/** Lists a grid's formatting rules and edits one; the grid repaints as rules change. */
export function FormattingDialog({ columns, rules, column, saved, onChange, onClose }: Props) {
  const first = column ?? columns[0]?.name ?? "";
  const [draft, setDraft] = useState<FormatRule>(() => newRule(first));
  const editing = rules.some((r) => r.id === draft.id);
  const ready = draft.column !== "" && complete(draft.condition);
  const setCondition = (condition: Condition) => setDraft({ ...draft, condition });
  const numeric = columns.find((c) => c.name === draft.column)?.numeric ?? false;
  // A fresh rule on the column just used.
  const reset = () => setDraft(newRule(draft.column));

  const save = () => {
    if (!ready) return;
    onChange(editing ? rules.map((r) => (r.id === draft.id ? draft : r)) : [...rules, draft]);
    reset();
  };
  const remove = (id: string) => {
    onChange(rules.filter((r) => r.id !== id));
    if (id === draft.id) reset();
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal formatting-dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Conditional formatting")}</h2>
        <p className="muted small">
          {t(saved ? "Rules color cells or rows; the first that matches wins. They are kept for this table." : "Rules color cells or rows; the first that matches wins. They last while this result is open.")}
        </p>
        <div className="formatting-rules" data-count={rules.length}>
          {rules.length === 0 && <p className="muted small formatting-empty">{t("No rules yet.")}</p>}
          {rules.map((rule, i) => (
            <div key={rule.id} className={`formatting-rule${rule.id === draft.id ? " selected" : ""}`}>
              <span
                className="formatting-sample"
                style={{
                  background:
                    rule.condition.kind === "scale"
                      ? `linear-gradient(90deg, ${colorOf(rule.color)}, ${scaleColor(rule.color, rule.condition.to, 1)})`
                      : colorOf(rule.color),
                }}
              />
              <button type="button" className="link formatting-text" onClick={() => setDraft(rule)} title={t("Edit")}>
                {describe(rule)}
              </button>
              <span className="formatting-target">{t(rule.row ? "Row" : "Cell")}</span>
              <span className="spacer" />
              <button type="button" onClick={() => onChange(moveRule(rules, i, -1))} disabled={i === 0} title={t("Move up")} aria-label={t("Move up")}>
                ↑
              </button>
              <button
                type="button"
                onClick={() => onChange(moveRule(rules, i, 1))}
                disabled={i === rules.length - 1}
                title={t("Move down")}
                aria-label={t("Move down")}
              >
                ↓
              </button>
              <button type="button" onClick={() => remove(rule.id)} title={t("Delete rule")} aria-label={t("Delete rule")}>
                ✕
              </button>
            </div>
          ))}
        </div>

        <h3>{t(editing ? "Edit rule" : "New rule")}</h3>
        <div className="formatting-form">
          <label className="field">
            <span>{t("Column")}</span>
            <select data-field="column" value={draft.column} onChange={(e) => setDraft({ ...draft, column: e.target.value })}>
              {columns.map((c, i) => (
                <option key={i} value={c.name}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>{t("Condition")}</span>
            <select
              data-field="condition"
              value={draft.condition.kind}
              onChange={(e) => setCondition(conditionOf(e.target.value as ConditionKind, draft.condition))}
            >
              {KINDS.filter((k) => !k.numeric || numeric || draft.condition.kind === k.kind).map((k) => (
                <option key={k.kind} value={k.kind}>
                  {t(k.label)}
                </option>
              ))}
            </select>
          </label>
          <ConditionFields condition={draft.condition} onChange={setCondition} onEnter={save} />
          <label className="field">
            <span>{t(draft.condition.kind === "scale" ? "From (lowest)" : "Color")}</span>
            <Swatches value={draft.color} onChange={(color) => setDraft({ ...draft, color })} name="color" />
          </label>
          {draft.condition.kind === "scale" ? (
            <label className="field">
              <span>{t("To (highest)")}</span>
              <Swatches value={draft.condition.to} onChange={(to) => setCondition({ kind: "scale", to })} name="to" />
            </label>
          ) : (
            <label className="field">
              <span>{t("Apply to")}</span>
              <select data-field="target" value={draft.row ? "row" : "cell"} onChange={(e) => setDraft({ ...draft, row: e.target.value === "row" })}>
                <option value="cell">{t("The cell")}</option>
                <option value="row">{t("The whole row")}</option>
              </select>
            </label>
          )}
        </div>
        <div className="actions">
          {editing && (
            <button type="button" onClick={reset}>
              {t("New rule")}
            </button>
          )}
          {rules.length > 0 && (
            <button type="button" onClick={() => onChange([])}>
              {t("Clear all")}
            </button>
          )}
          <span className="spacer" />
          <button type="button" data-action="save-rule" onClick={save} disabled={!ready}>
            {t(editing ? "Update rule" : "Add rule")}
          </button>
          <button type="button" className="primary" onClick={onClose} autoFocus>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}

function ConditionFields({
  condition,
  onChange,
  onEnter,
}: {
  condition: Condition;
  onChange: (c: Condition) => void;
  onEnter: () => void;
}) {
  const enter = (e: React.KeyboardEvent) => e.key === "Enter" && onEnter();
  switch (condition.kind) {
    case "equals":
    case "not_equals":
    case "contains":
      return (
        <label className="field">
          <span>{t("Value")}</span>
          <input
            data-field="value"
            value={condition.value}
            onChange={(e) => onChange({ ...condition, value: e.target.value })}
            onKeyDown={enter}
          />
        </label>
      );
    case "between":
      return (
        <div className="field">
          <span>{t("From / to (empty: no limit)")}</span>
          <div className="row tight">
            <input
              data-field="min"
              inputMode="decimal"
              placeholder={t("Min")}
              value={condition.min}
              onChange={(e) => onChange({ ...condition, min: e.target.value })}
              onKeyDown={enter}
            />
            <input
              data-field="max"
              inputMode="decimal"
              placeholder={t("Max")}
              value={condition.max}
              onChange={(e) => onChange({ ...condition, max: e.target.value })}
              onKeyDown={enter}
            />
          </div>
        </div>
      );
    default:
      return null;
  }
}

function Swatches({ value, onChange, name }: { value: ColorName; onChange: (c: ColorName) => void; name: string }) {
  return (
    <div className="swatches" role="radiogroup" data-field={name}>
      {COLOR_NAMES.map((c) => (
        <button
          type="button"
          key={c}
          role="radio"
          aria-checked={c === value}
          data-color={c}
          className={`swatch${c === value ? " selected" : ""}`}
          style={{ background: colorOf(c, 0.85) }}
          title={t(COLOR_LABELS[c])}
          onClick={() => onChange(c)}
        />
      ))}
    </div>
  );
}
