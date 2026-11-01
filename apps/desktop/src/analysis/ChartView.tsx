/** A result as a chart: the choice of type, columns and aggregation on the left, the chart on
 *  the right, recomputed as rows arrive; and the switch between grid and chart. */
import { type RefObject, useEffect, useRef, useState } from "react";
import { api, type ChartData, type ColumnInfo, toCommandError } from "../api";
import { locale, msg, t } from "../i18n";
import { workbench } from "../workbench";
import { Chart, useChartColors } from "./Chart";
import { type Aggregate, type ChartConfig, type ChartKind, autoConfig, toRequest, validConfig } from "./chartConfig";
import { exportChart } from "./chartExport";
import { categoryLabel, hasMarks, pieParts } from "./chartScale";

const KINDS: [ChartKind, string][] = [
  ["bar", msg("Bars")],
  ["line", msg("Lines")],
  ["scatter", msg("Scatter")],
  ["pie", msg("Pie")],
];
const AGGREGATES: [Aggregate, string][] = [
  ["sum", msg("Sum")],
  ["count", msg("Count")],
  ["avg", msg("Average")],
  ["min", msg("Minimum")],
  ["max", msg("Maximum")],
];
/** While rows arrive, the chart is computed again at most this often. */
const REFRESH_MS = 300;

interface Props {
  jobId: number;
  columns: ColumnInfo[];
  /** Rows loaded: more rows draw the chart again. */
  rows: number;
  /** The rows are all of the result's. */
  complete: boolean;
  /** `null` until chosen: the first choice is made from the columns. */
  config: ChartConfig | null;
  onConfig: (config: ChartConfig) => void;
  /** Without the panel of choices: a dashboard's widget, which has its own settings. */
  bare?: boolean;
}

/** The size of an element, followed as it changes. */
function useSize(ref: RefObject<HTMLElement | null>): { width: number; height: number } | null {
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setSize({ width: el.clientWidth, height: el.clientHeight }));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return size;
}

export function ChartView({ jobId, columns, rows, complete, config, onConfig, bare = false }: Props) {
  const valid = config ? validConfig(config, columns) : null;
  // A saved chart naming a column that is gone asks for the choice again.
  const stale = config !== null && valid === null;
  const shown = valid ?? autoConfig(columns);
  const request = stale ? null : toRequest(shown, columns);
  const requestKey = JSON.stringify(request);
  const [data, setData] = useState<ChartData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const drawn = useRef(false);
  // A throttle: new rows do not drop the request in flight, and one goes out at most every
  // REFRESH_MS. An answer counts while the chart still asks the same of the same result, and
  // only if no later answer came first.
  const target = `${jobId}|${requestKey}`;
  const current = useRef(target);
  current.current = target;
  const sentAt = useRef(0);
  const sent = useRef(0);
  const applied = useRef(0);
  useEffect(() => {
    if (!request) return setData(null);
    const wait = drawn.current ? Math.max(0, REFRESH_MS - (Date.now() - sentAt.current)) : 0;
    const timer = setTimeout(() => {
      sentAt.current = Date.now();
      const n = ++sent.current;
      const fresh = () => current.current === target && n > applied.current;
      api.chartData(jobId, request).then(
        (d) => {
          if (!fresh()) return;
          applied.current = n;
          drawn.current = true;
          setData(d);
          setError(null);
        },
        (e: unknown) => {
          if (!fresh()) return;
          applied.current = n;
          setError(toCommandError(e).message);
        },
      );
    }, wait);
    return () => clearTimeout(timer);
  }, [jobId, requestKey, rows]);

  const canvas = useRef<HTMLDivElement>(null);
  const size = useSize(canvas);
  const svg = useRef<SVGSVGElement>(null);
  const colors = useChartColors();
  const set = (patch: Partial<ChartConfig>) => onConfig({ ...shown, ...patch });
  const save = (format: "png" | "svg") => {
    if (!svg.current) return;
    void exportChart(svg.current, format).catch((e: unknown) =>
      workbench().showError(e instanceof Error ? e.message : toCommandError(e).message),
    );
  };
  const grouped = shown.kind === "bar" || shown.kind === "pie";
  const empty = data !== null && !hasMarks(data);
  const pieNotes = data && shown.kind === "pie" && !empty ? pieNotesOf(data, t("Other")) : [];

  return (
    <div className="chart-view">
      {!bare && (
      <div className="chart-side">
        <label>
          {t("Chart type")}
          <select aria-label={t("Chart type")} value={shown.kind} onChange={(e) => set({ kind: e.target.value as ChartKind })}>
            {KINDS.map(([kind, label]) => (
              <option key={kind} value={kind}>
                {t(label)}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("X axis")}
          <select
            aria-label={t("X axis")}
            value={shown.x}
            // The X column is no longer a series: drawn against itself, with no box to uncheck.
            onChange={(e) => set({ x: e.target.value, y: shown.y.filter((n) => n !== e.target.value) })}
          >
            {columns.map((c, i) => (
              <option key={i} value={c.name}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        <fieldset aria-label={t("Y axis")}>
          <legend>{t("Y axis")}</legend>
          {columns.map((c, i) =>
            c.name === shown.x ? null : (
              <label key={i} className="check">
                <input
                  type="checkbox"
                  checked={shown.y.includes(c.name)}
                  onChange={(e) => set({ y: e.target.checked ? [...shown.y, c.name] : shown.y.filter((n) => n !== c.name) })}
                />
                {c.name}
              </label>
            ),
          )}
        </fieldset>
        {grouped && (
          <label>
            {t("Aggregate")}
            <select aria-label={t("Aggregate")} value={shown.aggregate} onChange={(e) => set({ aggregate: e.target.value as Aggregate })}>
              {AGGREGATES.map(([aggregate, label]) => (
                <option key={aggregate} value={aggregate}>
                  {t(label)}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="chart-export">
          <span className="muted small">{t("Export")}</span>
          <button type="button" onClick={() => save("png")}>
            {t("PNG image…")}
          </button>
          <button type="button" onClick={() => save("svg")}>
            {t("SVG image…")}
          </button>
        </div>
      </div>
      )}
      <div className="chart-main">
        {stale && (
          <div className="notice warn">
            {t("The chart's columns are no longer in the result: choose them again.")}
            <button type="button" onClick={() => onConfig(shown)}>
              {t("Draw")}
            </button>
          </div>
        )}
        {error && <div className="notice error">{error}</div>}
        <div className="chart-canvas" ref={canvas}>
          {data && request && size && !empty && (
            <Chart ref={svg} data={data} kind={shown.kind} width={size.width} height={size.height} colors={colors}
              otherLabel={t("Other")}
              xName={shown.x}
              label={t("Chart of {series}", { series: data.series.map((s) => s.name).join(", ") })}
              locale={locale()}
            />
          )}
          {data && request && empty && (
            <div className="chart-empty muted">
              {data.rows === 0 ? t("No rows to draw.") : t("No data to draw: the chosen columns hold no numbers.")}
            </div>
          )}
        </div>
        {data && (!complete || data.reduced) && (
          <div className="chart-note muted small">
            {!complete &&
              t(data.rows === 1 ? "Drawn from the {n} row loaded" : "Drawn from the {n} rows loaded", {
                n: data.rows.toLocaleString(locale()),
              })}
            {!complete && data.reduced && " · "}
            {data.reduced && t("Some points or categories are grouped to fit.")}
          </div>
        )}
        {pieNotes.map((note) => (
          <div key={note} className="chart-note muted small">
            {note}
          </div>
        ))}
      </div>
    </div>
  );
}

/** What a pie does not show: the categories it leaves out, and the series past the first. */
function pieNotesOf(data: ChartData, otherLabel: string): string[] {
  const notes: string[] = [];
  const left = pieParts(data.series[0]?.values ?? []).left;
  if (left.length > 0) {
    const name = (i: number) => (data.other && i === data.categories.length - 1 ? otherLabel : categoryLabel(data.categories[i] ?? null));
    notes.push(t("Left out of the pie (zero, negative or no value): {list}", { list: left.map(name).join(", ") }));
  }
  if (data.series.length > 1) notes.push(t("A pie shows only its first series: {name}.", { name: data.series[0]?.name ?? "" }));
  return notes;
}

/** Grid or chart, in the result's status bar. */
export function ChartToggle({ chart, onChange }: { chart: boolean; onChange: (chart: boolean) => void }) {
  return (
    <span className="view-toggle" role="group" aria-label={t("Result view")}>
      <button type="button" className={chart ? "" : "toggled"} aria-pressed={!chart} onClick={() => onChange(false)}>
        {t("Grid")}
      </button>
      <button type="button" className={chart ? "toggled" : ""} aria-pressed={chart} onClick={() => onChange(true)}>
        {t("Chart")}
      </button>
    </span>
  );
}
