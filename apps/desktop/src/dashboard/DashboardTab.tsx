import { useEffect, useRef, useState } from "react";
import { api, toCommandError } from "../api";
import type { ChartConfig } from "../analysis/chartConfig";
import { msg, t } from "../i18n";
import type { TabBase, TabProps } from "../tabs/registry";
import { ask } from "../ui/ask";
import { workbench } from "../workbench";
import { WidgetForm } from "./WidgetForm";
import { WidgetView } from "./WidgetView";
import { dashboardFile, moveWidget, REFRESH_CHOICES, type Widget } from "./model";
import { idle, runWidget, type WidgetRun, WidgetSessions } from "./run";

export interface DashboardTabModel extends TabBase {
  kind: "dashboard";
  widgets: Widget[];
  /** Minutes between two refreshes while the tab is shown; 0 is off. */
  refresh: number;
  /** The name of its file in the data folder, once saved. */
  name: string | null;
}

declare module "../tabs/registry" {
  interface TabKinds {
    dashboard: DashboardTabModel;
  }
}

const REFRESH_LABELS = new Map<number, string>([
  [0, msg("Auto-refresh off")],
  [1, msg("Every minute")],
  [5, msg("Every 5 minutes")],
  [15, msg("Every 15 minutes")],
  [60, msg("Every hour")],
]);

/** A dashboard: a grid of widgets, each the result of a saved script. They run together on
 *  sessions of their own, refresh by hand or on a timer, and the dashboard is kept with the
 *  tab and as a file in the data folder. */
export function DashboardTab({ tab, connection, active, onChange }: TabProps<DashboardTabModel>) {
  const [runs, setRuns] = useState<Record<string, WidgetRun>>({});
  const [configuring, setConfiguring] = useState<ReadonlySet<string>>(() => new Set());
  const runsNow = useRef<Record<string, WidgetRun>>({});
  const tabNow = useRef(tab);
  tabNow.current = tab;
  const sessions = useRef(new WidgetSessions());
  // The latest run of each widget: an older one that ends later is ignored.
  const latest = useRef(new Map<string, number>());
  const gone = useRef(false);
  const project = tab.project ?? null;

  // The jobs and the sessions go with the tab.
  useEffect(() => {
    const held = sessions.current;
    gone.current = false;
    return () => {
      gone.current = true;
      for (const run of Object.values(runsNow.current)) if (run.result?.jobId != null) void api.closeQuery(run.result.jobId);
      held.closeAll();
    };
  }, []);

  const setRun = (id: string, patch: Partial<WidgetRun>) => {
    runsNow.current = { ...runsNow.current, [id]: { ...(runsNow.current[id] ?? idle), ...patch } };
    setRuns(runsNow.current);
  };

  /** Changes the dashboard; one kept in a file has changes not saved. */
  const edit = (patch: Partial<Pick<DashboardTabModel, "widgets" | "refresh" | "title">>) =>
    onChange({ ...patch, unsaved: tabNow.current.name !== null });

  /** Runs `widget` again; `scripts` is the list of the project's scripts, which a refresh of
   *  all widgets reads once. */
  function refresh(widget: Widget, scripts = api.listScripts(project)): Promise<void> {
    const n = (latest.current.get(widget.id) ?? 0) + 1;
    latest.current.set(widget.id, n);
    return runWidget(widget, {
      project,
      connection: tabNow.current.connectionId,
      scripts,
      sessions: sessions.current,
      current: () => latest.current.get(widget.id) === n && !gone.current,
      update: (patch) => setRun(widget.id, patch),
      previous: runsNow.current[widget.id]?.result?.jobId ?? null,
    });
  }

  /** Every widget that is not running now, in parallel. */
  function refreshAll() {
    const scripts = api.listScripts(project);
    // A list that cannot be read fails each widget with its reason.
    scripts.catch(() => undefined);
    return Promise.all(tabNow.current.widgets.filter((w) => !runsNow.current[w.id]?.running).map((w) => refresh(w, scripts)));
  }

  // The first time the tab is shown the widgets run; a restored dashboard shows fresh rows.
  const started = useRef(false);
  useEffect(() => {
    if (active && !started.current) {
      started.current = true;
      void refreshAll();
    }
  }, [active]);

  useEffect(() => {
    if (!active || tab.refresh <= 0) return;
    const timer = setInterval(() => void refreshAll(), tab.refresh * 60_000);
    return () => clearInterval(timer);
  }, [active, tab.refresh]);

  function openForm(initial: Widget | null) {
    workbench().openDialog((close) => (
      <WidgetForm
        initial={initial}
        project={project}
        connections={workbench().connections()}
        onClose={close}
        onSave={(widget) => {
          const widgets = tabNow.current.widgets;
          edit({ widgets: initial ? widgets.map((w) => (w.id === widget.id ? widget : w)) : [...widgets, widget] });
          if (!initial && widget.view === "chart") setConfiguring((c) => new Set(c).add(widget.id));
          void refresh(widget);
        }}
      />
    ));
  }

  function remove(id: string) {
    latest.current.set(id, (latest.current.get(id) ?? 0) + 1);
    const job = runsNow.current[id]?.result?.jobId;
    if (job != null) void api.closeQuery(job);
    const { [id]: _, ...rest } = runsNow.current;
    runsNow.current = rest;
    setRuns(rest);
    sessions.current.close(id);
    edit({ widgets: tabNow.current.widgets.filter((w) => w.id !== id) });
  }

  const changeChart = (id: string, config: ChartConfig) =>
    edit({ widgets: tabNow.current.widgets.map((w) => (w.id === id ? { ...w, chart: config } : w)) });

  const toggleConfiguring = (id: string) =>
    setConfiguring((c) => {
      const next = new Set(c);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  async function save() {
    const name = (tab.title ?? "").trim() || t("Dashboard");
    try {
      if (tab.name !== name) {
        const saved = (await api.listDashboards(project)).map((d) => d.name);
        // File systems may ignore case, so "Sales" and "sales" are one file; only the dashboard's own name is free.
        const taken = saved.some((n) => n.toLowerCase() === name.toLowerCase() && n !== tab.name);
        if (taken && !(await ask(t("A dashboard named {name} exists. Replace it?", { name }), { confirm: t("Replace"), danger: true }))) return;
        // Saved under another name before: the file follows its new name.
        if (!taken && tab.name !== null && saved.includes(tab.name)) await api.renameDashboard(project, tab.name, name);
      }
      const content = { widgets: tab.widgets, refresh: tab.refresh, connection: tab.connectionId };
      await api.saveDashboard(project, name, dashboardFile(name, content));
      onChange({ name, title: name, unsaved: false });
    } catch (e) {
      workbench().showError(toCommandError(e).message);
    }
  }

  return (
    <div className="object-tab dashboard-tab" style={{ display: active ? "flex" : "none" }}>
      <div className="toolbar dashboard-toolbar">
        <input
          className="dashboard-title"
          data-field="title"
          aria-label={t("Dashboard name")}
          placeholder={t("Dashboard")}
          value={tab.title ?? ""}
          onChange={(e) => edit({ title: e.target.value })}
        />
        <span className="muted small">{connection.name}</span>
        <span className="spacer" />
        <button type="button" onClick={() => openForm(null)} data-action="add">
          {t("Add widget")}
        </button>
        <button type="button" onClick={() => void refreshAll()} data-action="refresh_all">
          {t("Refresh all")}
        </button>
        <select
          aria-label={t("Auto-refresh")}
          data-field="auto_refresh"
          value={tab.refresh}
          onChange={(e) => edit({ refresh: Number(e.target.value) })}
        >
          {REFRESH_CHOICES.map((minutes) => (
            <option key={minutes} value={minutes}>
              {t(REFRESH_LABELS.get(minutes) ?? "")}
            </option>
          ))}
        </select>
        <button type="button" onClick={() => void save()} data-action="save">
          {t("Save")}
        </button>
      </div>
      <div className="dashboard-grid">
        {tab.widgets.map((widget, i) => (
          <WidgetView
            key={widget.id}
            widget={widget}
            run={runs[widget.id] ?? idle}
            first={i === 0}
            last={i === tab.widgets.length - 1}
            configuring={configuring.has(widget.id)}
            onRefresh={() => void refresh(widget)}
            onConfigure={() => toggleConfiguring(widget.id)}
            onChart={(config) => changeChart(widget.id, config)}
            onEdit={() => openForm(widget)}
            onMove={(by) => edit({ widgets: moveWidget(tab.widgets, widget.id, by) })}
            onRemove={() => remove(widget.id)}
          />
        ))}
        {tab.widgets.length === 0 && <p className="muted dashboard-empty">{t("An empty dashboard: add a widget.")}</p>}
      </div>
    </div>
  );
}
