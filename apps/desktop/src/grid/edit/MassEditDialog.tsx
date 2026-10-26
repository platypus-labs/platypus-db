// The dialogs of the selection's mass edits: one value for every cell, find and replace, a
// number sequence. Each shows what applying would do before it is applied; the work is done
// by the pure functions of `massEdit.ts`, find and replace in a worker (`replaceRunner.ts`).
import { type ReactNode, useEffect, useMemo, useState } from "react";
import type { CellInput } from "../../api";
import { msg, plural, t } from "../../i18n";
import {
  type Outcome,
  type Problem,
  type Rejection,
  type ReplaceJob,
  type Replaced,
  type Rules,
  type SelectedCell,
  counts,
  numberSequence,
  replaceInputs,
  replacedOutcome,
  replacer,
  setValue,
} from "./massEdit";
import { type ReplaceRun, type RunResult, type WorkerLike, runReplace } from "./replaceRunner";

export type MassEditKind = "set" | "replace" | "sequence";

const PROBLEMS: Record<Problem, string> = {
  integer: msg("not a whole number"),
  number: msg("not a number"),
  bool: msg("not true or false"),
  binary: msg("not hexadecimal bytes"),
  json: msg("not valid JSON"),
  enum: msg("not one of the column's values"),
  null: msg("the column does not take NULL"),
  default: msg("the column has no default and does not take NULL"),
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
  /** What applying does; `null` while the input is incomplete or being worked out. */
  outcome: Outcome | null;
  /** The input `outcome` was worked out from, in the summary's `data-input`: a sign that it is
   *  up to date. */
  input: string;
  /** Shown in place of the summary, e.g. an invalid regular expression. */
  problem?: string | null;
  /** Shown while there is no outcome yet, e.g. while matches are counted. */
  status?: string | null;
  /** The summary's first line, when not the count of changed cells. */
  headline?: string;
  names: readonly string[];
  onApply: (outcome: Outcome) => void;
  onClose: () => void;
}

/** The summary of what applying changes, and of the cells left as they are. */
function Summary({ outcome, input, headline, names }: { outcome: Outcome; input: string; headline?: string; names: readonly string[] }) {
  const changed = counts(outcome).changed;
  return (
    <div className="notice small mass-edit-summary" role="status" data-input={input}>
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
  );
}

/** The dialog around a mass edit: its fields, what applying changes, Cancel and Apply. */
function Frame({ title, children, outcome, input, problem, status, headline, names, onApply, onClose }: FrameProps) {
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
        <p className="muted small">{t("Only the rows loaded in the grid change.")}</p>
        {problem ? (
          <div className="notice error small">{problem}</div>
        ) : outcome ? (
          <Summary outcome={outcome} input={input} headline={headline} names={names} />
        ) : (
          status && (
            <div className="notice small" role="status">
              {status}
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
    <Frame title={t("Set value of the selection")} outcome={outcome} input={JSON.stringify([mode, text])} {...framed(props)}>
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

/** How far the count of a find and replace got. */
type Count =
  | { state: "idle" }
  | { state: "counting"; done: number; total: number }
  | { state: "slow" }
  | { state: "failed"; message: string }
  | { state: "done"; input: string; result: Replaced };

/** Typing waits this long before the matches are counted again. */
const DEBOUNCE_MS = 200;

const createWorker = () =>
  new Worker(new URL("./replaceWorker.ts", import.meta.url), { type: "module" }) as unknown as WorkerLike;

/**
 * The outcome of replacing as `job` says in `cells`, worked out in a worker once typing pauses:
 * the UI stays responsive whatever the expression, and one too slow is stopped. `null` job:
 * nothing to count.
 */
function useReplaceCount(cells: SelectedCell[], rules: Rules, job: Omit<ReplaceJob, "texts"> | null, input: string): Count {
  const inputs = useMemo(() => replaceInputs(cells, rules), [cells, rules]);
  const [count, setCount] = useState<Count>({ state: "idle" });
  useEffect(() => {
    if (!job) {
      setCount({ state: "idle" });
      return;
    }
    setCount({ state: "counting", done: 0, total: inputs.texts.length });
    let live = true;
    let run: ReplaceRun | undefined;
    const settled = (r: RunResult): Count =>
      "results" in r
        ? { state: "done", input, result: replacedOutcome(cells, rules, inputs.targets, r.results) }
        : "slow" in r
          ? { state: "slow" }
          : { state: "failed", message: r.error };
    const timer = setTimeout(() => {
      run = runReplace({ ...job, texts: inputs.texts }, createWorker, (done) => live && setCount({ state: "counting", done, total: inputs.texts.length }));
      void run.result.then((r) => live && setCount(settled(r)));
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
      run?.cancel();
    };
    // `input` stands for `job`, which is a new object on every render.
  }, [cells, rules, inputs, input]);
  return count;
}

/** The replace dialog's summary line: replacements and cells. */
function replacedHeadline(result: Replaced | null): string {
  if (!result || result.replacements === 0) return t("No matches.");
  if (result.replacements === 1) return t("1 replacement in 1 cell.");
  return plural(result.changes.length, "{replacements} replacements in {n} cell.", "{replacements} replacements in {n} cells.", {
    replacements: result.replacements.toLocaleString(),
  });
}

export function ReplaceDialog(props: Props) {
  const { cells, rules } = props;
  const [find, setFind] = useState("");
  const [replacement, setReplacement] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [regex, setRegex] = useState(false);
  const replace = useMemo(() => replacer(find, replacement, { caseSensitive, regex }), [find, replacement, caseSensitive, regex]);
  const input = JSON.stringify([find, replacement, caseSensitive, regex]);
  const valid = typeof replace === "function";
  const count = useReplaceCount(cells, rules, valid ? { find, replacement, caseSensitive, regex } : null, input);
  const result = count.state === "done" && count.input === input ? count.result : null;
  const problem =
    replace && "error" in replace
      ? t("Invalid regular expression: {message}", { message: replace.error })
      : count.state === "slow"
        ? t("The search takes too long on these cells: try a simpler expression.")
        : count.state === "failed"
          ? count.message
          : null;
  const status =
    valid && !result
      ? count.state === "counting" && count.done > 0
        ? t("Counting the matches… {done} of {total} cells", { done: count.done.toLocaleString(), total: count.total.toLocaleString() })
        : t("Counting the matches…")
      : null;
  return (
    <Frame
      title={t("Replace text in the selection")}
      outcome={result}
      input={input}
      problem={problem}
      status={status}
      headline={replacedHeadline(result)}
      {...framed(props)}
    >
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
      input={JSON.stringify([start, step])}
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
