import type { ChartConfig } from "../analysis/chartConfig";
import { describe, ResultPanel } from "../components/ResultPanel";
import { locale, t } from "../i18n";
import { Icon } from "../ui/Icon";
import { widgetTitle, type Widget } from "./model";
import type { WidgetRun } from "./run";

interface Props {
  widget: Widget;
  run: WidgetRun;
  first: boolean;
  last: boolean;
  /** A chart widget shows the choice of its type and columns. */
  configuring: boolean;
  onRefresh: () => void;
  onConfigure: () => void;
  onChart: (config: ChartConfig) => void;
  onEdit: () => void;
  onMove: (by: -1 | 1) => void;
  onRemove: () => void;
}

/** What a widget shows under its title: the rows as a grid or a chart, the one value, or why
 *  there is nothing. */
function Body({ widget, run, configuring, onChart }: Pick<Props, "widget" | "run" | "configuring" | "onChart">) {
  if (run.error) {
    return (
      <div className="notice error small dashboard-error" data-field="error">
        {run.error}
      </div>
    );
  }
  if (widget.view === "value") {
    if (run.at === null) return <p className="muted small dashboard-wait">{t("Executing…")}</p>;
    return run.value === null ? (
      <p className="muted dashboard-wait" data-field="value">
        {t("No rows")}
      </p>
    ) : (
      <div className="dashboard-value" data-field="value" title={run.value}>
        {run.value}
      </div>
    );
  }
  if (!run.result) return <p className="muted small dashboard-wait">{t("Executing…")}</p>;
  return (
    <ResultPanel
      result={run.result}
      chart={widget.view === "chart" ? { config: widget.chart, onConfig: onChart, bare: !configuring } : null}
    />
  );
}

/** One widget of a dashboard: its title and tools, then its body and the state of its run. */
export function WidgetView(props: Props) {
  const { widget, run, first, last } = props;
  const status = run.error ? t("Error") : run.running ? t("Executing…") : run.at !== null ? t("Updated {time}", { time: new Date(run.at).toLocaleTimeString(locale()) }) : "";
  return (
    <section
      className={`dashboard-widget${run.running ? " running" : ""}`}
      data-widget={widget.id}
      data-title={widgetTitle(widget)}
      style={{ gridColumn: `span ${widget.width}`, gridRow: `span ${widget.height}` }}
    >
      <div className="dashboard-widget-bar">
        <span className="dashboard-widget-title" title={widget.script}>
          {widgetTitle(widget)}
        </span>
        <span className="spacer" />
        {widget.view === "chart" && (
          <button
            type="button"
            className={props.configuring ? "toggled" : ""}
            aria-pressed={props.configuring}
            onClick={props.onConfigure}
            data-action="chart_settings"
          >
            {t("Chart settings")}
          </button>
        )}
        <button type="button" className="icon" disabled={run.running} onClick={props.onRefresh} aria-label={t("Refresh")} title={t("Refresh")} data-action="refresh">
          <Icon name="refresh" />
        </button>
        <button type="button" onClick={props.onEdit} data-action="edit">
          {t("Edit")}
        </button>
        <button type="button" className="icon" disabled={first} onClick={() => props.onMove(-1)} aria-label={t("Move up")} title={t("Move up")} data-action="up">
          ↑
        </button>
        <button type="button" className="icon" disabled={last} onClick={() => props.onMove(1)} aria-label={t("Move down")} title={t("Move down")} data-action="down">
          ↓
        </button>
        <button type="button" className="icon" onClick={props.onRemove} aria-label={t("Remove")} title={t("Remove")} data-action="remove">
          <Icon name="delete" />
        </button>
      </div>
      <div className="dashboard-body">
        <Body widget={widget} run={run} configuring={props.configuring} onChart={props.onChart} />
      </div>
      <div className="dashboard-widget-status small muted" data-field="status" data-at={run.at ?? ""}>
        <span>{status}</span>
        {run.result?.status.kind === "rows" && widget.view !== "value" && <span>{describe(run.result)}</span>}
      </div>
    </section>
  );
}
