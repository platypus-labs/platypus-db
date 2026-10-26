// The dialogs of the selection's mass edits: one value for every cell, find and replace, a
// number sequence. Each shows what applying would do before it is applied; the work is done
// by the pure functions of `massEdit.ts`.
import { type ReactNode, useMemo, useState } from "react";
import type { CellInput } from "../../api";
import { msg, plural, t } from "../../i18n";
import {
  type Outcome,
  type Problem,
  type Rejection,
  type Rules,
  type SelectedCell,
  counts,
  numberSequence,
  replaceText,
  replacer,
  setValue,
} from "./massEdit";

export type MassEditKind = "set" | "replace" | "sequence";

const PROBLEMS: Record<Problem, string> = {
  integer: msg("not a whole number"),
  number: msg("not a number"),
  bool: msg("not true or false"),
  binary: msg("not hexadecimal bytes"),
  json: msg("not valid JSON"),
  enum: msg("not one of the column's values"),
  null: msg("the column does not take NULL"),
  readonly: msg("not a column of the table"),
  not_number: msg("not a number column"),
};

/** Each column's cells left as they were, and why, e.g. `id: 2 cells, not a whole number`. */
function rejectedLines(rejected: Rejection[], names: readonly string[]): string[] {
  return rejected.map((r) =>
    plural(r.cells, "{column}: {n} cell, {reason}", "{column}: {n} cells, {reason}", {
      column: names[r.col] ?? "?",
      reason: t(PROBLEMS[r.problem]),
    }),
  );
}

/** The edit bar's report of cells a mass edit left as they were; `null` when there are none. */
export function rejectedReport(rejected: Rejection[], names: readonly string[]): string | null {
  const total = rejected.reduce((n, r) => n + r.cells, 0);
  if (total === 0) return null;
  return plural(total, "{n} cell not changed ({reasons})", "{n} cells not changed ({reasons})", {
    reasons: rejectedLines(rejected, names).join("; "),
  });
}

interface FrameProps {
  title: string;
  children: ReactNode;
  /** What applying does; `null` while the input is incomplete. */
  outcome: Outcome | null;
  /** Shown in place of the summary, e.g. an invalid regular expression. */
  problem?: string | null;
  /** The summary's first line, when not the count of changed cells. */
  headline?: string;
  names: readonly string[];
  onApply: (outcome: Outcome) => void;
  onClose: () => void;
}

/** The dialog around a mass edit: its fields, what applying changes, Cancel and Apply. */
function Frame({ title, children, outcome, problem, headline, names, onApply, onClose }: FrameProps) {
  const changed = outcome ? counts(outcome).changed : 0;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal mass-edit"
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          if (outcome && changed > 0) onApply(outcome);
        }}
      >
        <h2>{title}</h2>
        {children}
        {problem ? (
          <div className="notice error small">{problem}</div>
        ) : (
          outcome && (
            <div className="notice small mass-edit-summary" role="status">
              <div>{headline ?? plural(changed, "{n} cell will change.", "{n} cells will change.")}</div>
              {outcome.rejected.length > 0 && (
                <>
                  <div>{t("Left as they are:")}</div>
                  <ul>
                    {rejectedLines(outcome.rejected, names).map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                </>
              )}
            </div>
          )
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={!outcome || changed === 0}>
            {t("Apply")}
          </button>
        </div>
      </form>
    </div>
  );
}

interface Props {
  cells: SelectedCell[];
  rules: Rules;
  /** Result column names, by index. */
  names: readonly string[];
  onApply: (outcome: Outcome) => void;
  onClose: () => void;
}

/** The props a dialog hands on to its frame. */
const framed = ({ names, onApply, onClose }: Props) => ({ names, onApply, onClose });

type SetMode = "value" | "null" | "default";

export function SetValueDialog(props: Props) {
  const { cells, rules } = props;
  const [mode, setMode] = useState<SetMode>("value");
  const [text, setText] = useState("");
  const outcome = useMemo(() => {
    const input: CellInput = mode === "value" ? { kind: "value", text } : { kind: mode };
    return setValue(cells, rules, input);
  }, [cells, rules, mode, text]);
  const choice = (value: SetMode, label: string) => (
    <label className="check">
      <input type="radio" name="mass-set-mode" checked={mode === value} onChange={() => setMode(value)} />
      {label}
    </label>
  );
  return (
    <Frame title={t("Set value of the selection")} outcome={outcome} {...framed(props)}>
      <div className="row mass-edit-modes">
        {choice("value", t("Value"))}
        {choice("null", "NULL")}
        {choice("default", "DEFAULT")}
      </div>
      <input
        autoFocus
        aria-label={t("Value")}
        value={text}
        disabled={mode !== "value"}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
      />
      <p className="muted small">{t("Each cell is checked against its column's type.")}</p>
    </Frame>
  );
}

export function ReplaceDialog(props: Props) {
  const { cells, rules } = props;
  const [find, setFind] = useState("");
  const [replacement, setReplacement] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [regex, setRegex] = useState(false);
  const replace = useMemo(() => replacer(find, replacement, { caseSensitive, regex }), [find, replacement, caseSensitive, regex]);
  const result = useMemo(() => (typeof replace === "function" ? replaceText(cells, rules, replace) : null), [cells, rules, replace]);
  const problem = replace && "error" in replace ? t("Invalid regular expression: {message}", { message: replace.error }) : null;
  const headline =
    !result || result.replacements === 0
      ? t("No matches.")
      : result.replacements === 1
        ? t("1 replacement in 1 cell.")
        : plural(result.changes.length, "{replacements} replacements in {n} cell.", "{replacements} replacements in {n} cells.", {
            replacements: result.replacements.toLocaleString(),
          });
  return (
    <Frame title={t("Replace text in the selection")} outcome={result} problem={problem} headline={headline} {...framed(props)}>
      <label className="field">
        <span>{t("Find")}</span>
        <input autoFocus value={find} spellCheck={false} onChange={(e) => setFind(e.target.value)} />
      </label>
      <label className="field">
        <span>{t("Replace with")}</span>
        <input value={replacement} spellCheck={false} onChange={(e) => setReplacement(e.target.value)} />
      </label>
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={caseSensitive} onChange={(e) => setCaseSensitive(e.target.checked)} />
          {t("Case sensitive")}
        </label>
        <label className="check">
          <input type="checkbox" checked={regex} onChange={(e) => setRegex(e.target.checked)} />
          {t("Regular expression")}
        </label>
      </div>
      <p className="muted small">{t("Only text cells are searched; NULL cells stay as they are.")}</p>
    </Frame>
  );
}

export function SequenceDialog(props: Props) {
  const { cells, rules } = props;
  const [start, setStart] = useState("1");
  const [step, setStep] = useState("1");
  const outcome = useMemo(() => numberSequence(cells, rules, start, step), [cells, rules, start, step]);
  return (
    <Frame
      title={t("Number sequence")}
      outcome={outcome}
      problem={outcome ? null : t("Start and step must be numbers.")}
      {...framed(props)}
    >
      <div className="row">
        <label className="field">
          <span>{t("Start")}</span>
          <input autoFocus inputMode="decimal" value={start} spellCheck={false} onChange={(e) => setStart(e.target.value)} />
        </label>
        <label className="field">
          <span>{t("Step")}</span>
          <input inputMode="decimal" value={step} spellCheck={false} onChange={(e) => setStep(e.target.value)} />
        </label>
      </div>
      <p className="muted small">{t("Down each selected number column, from the top row.")}</p>
    </Frame>
  );
}

export const MASS_EDIT_DIALOGS: Record<MassEditKind, (props: Props) => ReactNode> = {
  set: SetValueDialog,
  replace: ReplaceDialog,
  sequence: SequenceDialog,
};
