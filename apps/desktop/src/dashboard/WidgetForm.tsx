import { useEffect, useState } from "react";
import { api, type SavedConnection, type ScriptEntry, toCommandError } from "../api";
import { msg, t } from "../i18n";
import { DEFAULT_ROWS, MAX_HEIGHT, MAX_WIDTH, newWidget, type Widget, type WidgetView } from "./model";

const VIEW_LABELS: [WidgetView, string][] = [
  ["table", msg("Table")],
  ["chart", msg("Chart")],
  ["value", msg("Single value")],
];

interface Props {
  /** The widget to change, `null` to add one. */
  initial: Widget | null;
  project: string | null;
  connections: SavedConnection[];
  onSave: (widget: Widget) => void;
  onClose: () => void;
}

const range = (n: number) => Array.from({ length: n }, (_, i) => i + 1);

/** The form of a widget: its script, how it shows the result, and how much room it takes.
 *  A chart's columns are chosen on the widget itself, over its rows. */
export function WidgetForm({ initial, project, connections, onSave, onClose }: Props) {
  const [scripts, setScripts] = useState<ScriptEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [widget, setWidget] = useState<Widget>(() => initial ?? newWidget());
  const set = (patch: Partial<Widget>) => setWidget((w) => ({ ...w, ...patch }));

  useEffect(() => {
    api.listScripts(project).then(
      (list) => setScripts(list.entries.filter((e) => !e.folder)),
      (e: unknown) => {
        setScripts([]);
        setError(toCommandError(e).message);
      },
    );
  }, [project]);

  // A widget whose script is gone keeps it in the list, so that the form does not change it.
  const known = scripts?.some((s) => s.path === widget.script) ?? true;
  const ready = widget.script !== "" && scripts !== null;

  function save() {
    // The chart is drawn again from the new columns when the script changes.
    const changed = initial !== null && initial.script !== widget.script;
    onSave({ ...widget, title: widget.title.trim(), chart: changed ? null : widget.chart });
    onClose();
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal dashboard-form"
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          if (ready) save();
        }}
      >
        <h2>{t(initial ? "Edit widget" : "Add widget")}</h2>
        <label className="field">
          <span>{t("Script")}</span>
          <select data-field="script" autoFocus value={widget.script} onChange={(e) => set({ script: e.target.value })}>
            <option value="">{scripts === null ? t("Loading…") : t("Choose a saved script")}</option>
            {!known && <option value={widget.script}>{t("{path} (missing)", { path: widget.script })}</option>}
            {(scripts ?? []).map((s) => (
              <option key={s.path} value={s.path}>
                {s.path}
              </option>
            ))}
          </select>
        </label>
        {scripts?.length === 0 && <p className="muted small">{t("No saved scripts: create one in the Scripts panel.")}</p>}
        <label className="field">
          <span>{t("Title")}</span>
          <input data-field="title" value={widget.title} placeholder={t("The script's name")} onChange={(e) => set({ title: e.target.value })} />
        </label>
        <label className="field">
          <span>{t("Connection")}</span>
          <select data-field="connection" value={widget.connection ?? ""} onChange={(e) => set({ connection: e.target.value || null })}>
            <option value="">{t("The script's connection")}</option>
            {connections.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        <div className="row">
          <label className="field grow">
            <span>{t("Show as")}</span>
            <select data-field="view" value={widget.view} onChange={(e) => set({ view: e.target.value as WidgetView })}>
              {VIEW_LABELS.map(([view, label]) => (
                <option key={view} value={view}>
                  {t(label)}
                </option>
              ))}
            </select>
          </label>
          {widget.view === "table" && (
            <label className="field">
              <span>{t("Rows shown")}</span>
              <input
                data-field="rows"
                type="number"
                min={1}
                max={10000}
                value={widget.rows}
                onChange={(e) => set({ rows: Number(e.target.value) || DEFAULT_ROWS })}
              />
            </label>
          )}
        </div>
        {widget.view === "chart" && <p className="muted small">{t("Choose the chart's type and columns with its settings button, once the widget has its rows.")}</p>}
        <div className="row">
          <label className="field grow">
            <span>{t("Width (columns)")}</span>
            <select data-field="width" value={widget.width} onChange={(e) => set({ width: Number(e.target.value) })}>
              {range(MAX_WIDTH).map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          <label className="field grow">
            <span>{t("Height (rows)")}</span>
            <select data-field="height" value={widget.height} onChange={(e) => set({ height: Number(e.target.value) })}>
              {range(MAX_HEIGHT).map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        </div>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" data-action="save" disabled={!ready}>
            {t(initial ? "Save" : "Add")}
          </button>
        </div>
      </form>
    </div>
  );
}
