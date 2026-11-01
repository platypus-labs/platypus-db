import { useEffect, useRef } from "react";
import type { CompletionCatalog, DriverKind, Explain } from "../api";
import type { ChartConfig } from "../analysis/chartConfig";
import { ChartToggle } from "../analysis/ChartView";
import { describe, type Result, ResultPanel } from "../components/ResultPanel";
import { SqlEditor } from "../editor/SqlEditor";
import { t } from "../i18n";
import { Icon } from "../ui/Icon";
import { LoadingState } from "../ui/State";
import { Markdown } from "./Markdown";
import { type NotebookCell as Cell, planLines } from "./model";

/** What a cell's last run left: its result, its plan, or why it did not run. */
export interface CellRun {
  result: Result | null;
  plan: Explain | null;
  /** Why the cell did not run (a parameter without value, a refusal). */
  error: string | null;
  /** Why its plan could not be had: the run may still have gone. */
  planError: string | null;
  running: boolean;
}

export const idle: CellRun = { result: null, plan: null, error: null, planError: null, running: false };

interface Props {
  cell: Cell;
  /** `c1`… for a SQL cell. */
  name: string | null;
  run: CellRun;
  first: boolean;
  last: boolean;
  /** A text cell shows its source to edit. */
  editing: boolean;
  driver: DriverKind;
  catalog: CompletionCatalog | null;
  connectionName: string;
  onChange: (patch: Partial<Cell>) => void;
  onRun: () => void;
  onStop: () => void;
  onPlan: () => void;
  onEdit: (editing: boolean) => void;
  onMove: (by: -1 | 1) => void;
  onDelete: () => void;
  onSave: () => void;
  onFocus: () => void;
}

/** The plan of a cell, one step per line. */
function PlanList({ plan }: { plan: Explain }) {
  return (
    <ol className="notebook-plan" aria-label={t("Plan")}>
      {planLines(plan.root).map((line, i) => (
        <li key={i} style={{ paddingLeft: line.depth * 16 }}>
          <strong>{line.operation}</strong>
          {line.detail && <span className="muted"> {line.detail}</span>}
          {line.rows !== null && <span className="muted small"> · {t("{n} rows", { n: line.rows })}</span>}
          {line.cost !== null && <span className="muted small"> · {t("cost {n}", { n: line.cost })}</span>}
        </li>
      ))}
    </ol>
  );
}

/** A text cell: its Markdown, or its source while edited. */
function TextBody({ cell, editing, onChange, onEdit }: Pick<Props, "cell" | "editing" | "onChange" | "onEdit">) {
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (editing) area.current?.focus();
  }, [editing]);
  if (editing) {
    return (
      <textarea
        ref={area}
        className="notebook-text"
        value={cell.text}
        rows={Math.max(3, cell.text.split("\n").length + 1)}
        spellCheck
        aria-label={t("Text (Markdown)")}
        placeholder={t("Write in Markdown… Esc to finish")}
        onChange={(e) => onChange({ text: e.target.value })}
        onKeyDown={(e) => {
          if (e.key === "Escape" || (e.key === "Enter" && (e.ctrlKey || e.metaKey))) {
            e.preventDefault();
            onEdit(false);
          }
        }}
      />
    );
  }
  return (
    <div className="notebook-rendered" onDoubleClick={() => onEdit(true)} title={t("Double-click to edit")}>
      {cell.text.trim() ? <Markdown text={cell.text} /> : <p className="muted">{t("Empty text: double-click to write.")}</p>}
    </div>
  );
}

/** A SQL cell's result: the grid or the chart, its status, and its plan. */
function SqlOutput({ cell, run, onChange }: Pick<Props, "cell" | "run" | "onChange">) {
  const { result } = run;
  const rows = result?.status.kind === "rows";
  const chart = cell.chart;
  return (
    <>
      {(run.error ?? run.planError) && <div className="notice error small" data-field="error">{run.error ?? run.planError}</div>}
      {result && (
        <>
          <div className="notebook-status small muted" data-field="status" data-result={result.id}>
            <span>{describe(result)}</span>
            <span className="spacer" />
            {rows && (
              <ChartToggle
                chart={!!chart?.shown}
                onChange={(shown) => onChange({ chart: { shown, config: chart?.config ?? null } })}
              />
            )}
          </div>
          {(rows || result.status.kind === "error" || result.messages?.length) && (
            <div
              className={`notebook-result${rows ? (chart?.shown ? " chart" : "") : " short"}`}
              // A few rows take the room they need, the header and a scroll bar's included.
              style={rows && !chart?.shown ? { height: Math.min(260, 64 + result.rows * 24) } : undefined}
            >
              <ResultPanel
                result={result}
                chart={
                  rows && chart?.shown
                    ? {
                        config: (chart.config as ChartConfig | null) ?? null,
                        onConfig: (config) => onChange({ chart: { shown: true, config } }),
                      }
                    : null
                }
              />
            </div>
          )}
        </>
      )}
      {cell.plan && run.plan && <PlanList plan={run.plan} />}
    </>
  );
}

/** One cell of a notebook: its tools, then its SQL and result, or its text. */
export function NotebookCellView(props: Props) {
  const { cell, name, run, first, last, onChange, onRun, onPlan, onMove, onDelete, onEdit } = props;
  const sql = cell.kind === "sql";
  return (
    <section className={`notebook-cell ${cell.kind}${run.running ? " running" : ""}`} data-cell={cell.id} data-name={name ?? undefined}>
      <div className="notebook-cell-bar">
        {sql ? <span className="notebook-name">{name}</span> : <span className="notebook-name muted">{t("Text")}</span>}
        {sql && (
          <label className="check small" title={t("Query the rows the other cells loaded, as the tables c1, c2…, without running them again")}>
            <input type="checkbox" checked={cell.onResults} data-field="on_results" onChange={(e) => onChange({ onResults: e.target.checked })} />
            {t("On the results c1, c2…")}
          </label>
        )}
        {sql && !cell.onResults && <span className="muted small">{props.connectionName}</span>}
        <span className="spacer" />
        {sql && (
          <>
            <button type="button" className={cell.plan ? "toggled" : ""} aria-pressed={cell.plan} onClick={onPlan} data-action="plan">
              {t("Plan")}
            </button>
            {run.running ? (
              <button type="button" onClick={props.onStop} data-action="stop" title={t("Cancel the running statement")}>
                {t("Stop")}
              </button>
            ) : (
              <button type="button" className="primary" onClick={onRun} data-action="run" title={t("Run (Ctrl+Enter)")}>
                <Icon name="run" />
                {t("Run")}
              </button>
            )}
          </>
        )}
        {!sql && (
          <button type="button" onClick={() => onEdit(!props.editing)} data-action="edit">
            {t(props.editing ? "Done" : "Edit")}
          </button>
        )}
        <button type="button" className="icon" disabled={first} onClick={() => onMove(-1)} aria-label={t("Move up")} title={t("Move up")} data-action="up">
          <Icon name="arrow-up" />
        </button>
        <button type="button" className="icon" disabled={last} onClick={() => onMove(1)} aria-label={t("Move down")} title={t("Move down")} data-action="down">
          <Icon name="arrow-down" />
        </button>
        <button type="button" className="icon" onClick={onDelete} aria-label={t("Delete cell")} title={t("Delete cell")} data-action="delete">
          <Icon name="delete" />
        </button>
      </div>
      {sql ? (
        <>
          <div className="notebook-editor" onFocus={props.onFocus}>
            <SqlEditor
              // The dialect follows where the cell runs.
              key={props.driver}
              initialValue={cell.text}
              driver={props.driver}
              catalog={props.catalog}
              placeholder={t("SELECT … — Ctrl+Enter to run")}
              onChange={(text) => onChange({ text })}
              onRun={onRun}
              onSave={props.onSave}
            />
          </div>
          {run.running && <LoadingState compact className="notebook-running" title={t("Running…")} />}
          <SqlOutput cell={cell} run={run} onChange={onChange} />
        </>
      ) : (
        <TextBody cell={cell} editing={props.editing} onChange={onChange} onEdit={onEdit} />
      )}
    </section>
  );
}
