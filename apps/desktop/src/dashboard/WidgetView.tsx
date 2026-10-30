import { useCallback, useState } from "react";
import type { ChartConfig } from "../analysis/chartConfig";
import { ContextMenu, type MenuState } from "../components/ContextMenu";
import { describe, ResultPanel } from "../components/ResultPanel";
import { locale, t } from "../i18n";
import { toCommandError } from "../api";
import { workbench } from "../workbench";
import { Icon } from "../ui/Icon";
import "./actions";
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
    // A refresh clears the value but keeps `at`: it must not read "No rows" while it runs.
    if (run.at === null || run.running) return <p className="muted small dashboard-wait">{t("Executing…")}</p>;
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
  const { widget, run } = props;
  const [menu, setMenu] = useState<MenuState | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  /** Opens the menu under the button. */
  function openMenu(e: React.MouseEvent<HTMLButtonElement>) {
    const r = e.currentTarget.getBoundingClientRect();
    setMenu({
      x: r.left,
      y: r.bottom,
      target: { scope: "dashboard.widget", arg: { ...props } },
    });
  }
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
        <span className="dashboard-widget-source" title={widget.script}>
          {widget.script.split(/[/\\]/).pop()}
        </span>
        <span className="spacer" />
        <button
          type="button"
          className="icon"
          aria-haspopup="menu"
          aria-expanded={menu !== null}
          aria-label={t("Widget actions")}
          title={t("Widget actions")}
          onClick={openMenu}
          data-action="menu"
        >
          <Icon name="more" />
        </button>
      </div>
      <div className="dashboard-body">
        <Body widget={widget} run={run} configuring={props.configuring} onChart={props.onChart} />
      </div>
      <div className="dashboard-widget-status small muted" data-field="status" data-at={run.at ?? ""}>
        <span>{status}</span>
        {run.result?.status.kind === "rows" && widget.view !== "value" && <span>{describe(run.result)}</span>}
      </div>
      {menu && <ContextMenu menu={menu} onClose={closeMenu} onError={(e) => workbench().showError(toCommandError(e).message)} />}
    </section>
  );
}
