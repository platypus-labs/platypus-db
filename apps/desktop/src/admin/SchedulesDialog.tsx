/** "Scheduled tasks": scripts, exports and backups that run at set times while the app is
 *  open, each with its next run and its last runs; and the form of one, new or edited. */
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import {
  api,
  type BackupFormat,
  type Schedule,
  type ScheduleExportFormat,
  type ScheduleRepeat,
  type ScheduleTask,
  type ScheduleView,
  keepsDataIn,
  toCommandError,
} from "../api";
import { locale, msg, t } from "../i18n";
import { answerFor } from "../testAnswers";
import { ask } from "../ui/ask";
import { workbench } from "../workbench";
import { cleaned, newSchedule, repeatOf, sortSchedules, taskKinds, taskOf, taskOnConnection, writesFiles } from "./schedules";

const KINDS: Record<ScheduleTask["kind"], string> = {
  script: msg("Run a SQL script"),
  export: msg("Export a query"),
  backup: msg("Back up the database"),
};

const EVERY: [ScheduleRepeat["every"], string][] = [
  ["hour", msg("Every hour")],
  ["day", msg("Every day")],
  ["week", msg("Every week")],
  ["month", msg("Every month")],
];

/** The short names of the days, Monday first, in the interface's language. */
function weekdays(): string[] {
  const format = new Intl.DateTimeFormat(locale(), { weekday: "short", timeZone: "UTC" });
  // 9 November 2026 is a Monday.
  return Array.from({ length: 7 }, (_, i) => format.format(new Date(Date.UTC(2026, 10, 9 + i))));
}

/** The short names of `days`, 1 for Monday. */
function dayNames(days: number[]): string {
  const names = weekdays();
  return days.map((d) => names[d - 1]).join(", ");
}

/** When a task runs, in words: "Every day at 07:00 (Europe/Rome)". */
function describeWhen(s: Schedule): string {
  const r = s.repeat;
  const text =
    r.every === "hour"
      ? t("Every hour at minute {minute}", { minute: s.time.slice(3, 5) })
      : r.every === "day"
        ? t("Every day at {time}", { time: s.time })
        : r.every === "week"
          ? t("Every {days} at {time}", { days: dayNames(r.days), time: s.time })
          : t("On day {day} of every month at {time}", { day: r.day, time: s.time });
  return s.timezone ? `${text} (${s.timezone})` : text;
}

/** A run's outcome: done or failed, then `children`. */
function RunBadge({ ok, title, children }: { ok: boolean; title?: string; children?: ReactNode }) {
  return (
    <span className={`admin-badge ${ok ? "running" : "waiting"}`} title={title}>
      <span className="admin-dot" />
      {ok ? t("done") : t("failed")}
      {children}
    </span>
  );
}

const when = (iso: string) => new Date(iso).toLocaleString(locale(), { dateStyle: "medium", timeStyle: "short" });

/** The tasks of every connection; with `connectionId`, a new task starts on it. */
export function SchedulesDialog({ connectionId, onClose }: { connectionId?: string; onClose: () => void }) {
  const [schedules, setSchedules] = useState<ScheduleView[] | null>(null);
  const [editing, setEditing] = useState<Schedule | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [opened, setOpened] = useState<string | null>(null);
  const connections = workbench().connections();
  const nameOf = (id: string) => connections.find((c) => c.id === id)?.name ?? t("a deleted connection");

  useEffect(() => {
    const load = () => api.listSchedules().then(setSchedules, (e) => setError(toCommandError(e).message));
    void load();
    const off = listen("schedules-changed", () => void load());
    if (connectionId) setEditing(newSchedule(connectionId, crypto.randomUUID()));
    return () => void off.then((unlisten) => unlisten());
  }, []);

  async function remove(s: ScheduleView) {
    if (!(await ask(t("Delete the scheduled task {name}?", { name: s.name }), { confirm: t("Delete"), danger: true }))) return;
    try {
      setSchedules(await api.deleteSchedule(s.id));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  async function toggle(s: ScheduleView, enabled: boolean) {
    try {
      setSchedules(await api.saveSchedule({ ...strip(s), enabled }));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const close = () => (editing && !connectionId ? setEditing(null) : onClose());
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal schedules-dialog" role="dialog" onKeyDown={(e) => e.key === "Escape" && close()}>
        {editing ? (
          <ScheduleForm
            initial={editing}
            onSaved={(all) => {
              setSchedules(all);
              if (connectionId) onClose();
              else setEditing(null);
            }}
            onCancel={close}
          />
        ) : (
          <>
            <h2>{t("Scheduled tasks")}</h2>
            <p className="muted small">
              {t("They run while PlatypusDB is open; a time missed while it was closed runs at the next start.")}
            </p>
            {error && <div className="notice error small">{error}</div>}
            {schedules && schedules.length === 0 && <p className="muted small schedules-empty">{t("No scheduled tasks yet.")}</p>}
            {schedules && schedules.length > 0 && (
              <div className="schedules-list">
                <table className="meta-table admin-table">
                  <thead>
                    <tr>
                      <th>{t("Task")}</th>
                      <th>{t("When")}</th>
                      <th>{t("Next run")}</th>
                      <th>{t("Last run")}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {sortSchedules(schedules).map((s) => (
                      <ScheduleRow
                        key={s.id}
                        schedule={s}
                        connection={nameOf(s.connection_id)}
                        opened={opened === s.id}
                        onOpen={() => setOpened(opened === s.id ? null : s.id)}
                        onToggle={(on) => void toggle(s, on)}
                        onRun={() => void api.runScheduleNow(s.id).catch((e) => setError(toCommandError(e).message))}
                        onEdit={() => setEditing(strip(s))}
                        onDelete={() => void remove(s)}
                      />
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <div className="actions">
              <button type="button" onClick={() => setEditing(newSchedule(connections[0]?.id ?? "", crypto.randomUUID()))} disabled={connections.length === 0}>
                {t("New task…")}
              </button>
              <span className="spacer" />
              <button type="button" className="primary" onClick={onClose}>
                {t("Close")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** The task without what the list adds to it. */
function strip({ next: _next, runs: _runs, running: _running, ...schedule }: ScheduleView): Schedule {
  return schedule;
}

function ScheduleRow({
  schedule: s,
  connection,
  opened,
  onOpen,
  onToggle,
  onRun,
  onEdit,
  onDelete,
}: {
  schedule: ScheduleView;
  connection: string;
  opened: boolean;
  onOpen: () => void;
  onToggle: (on: boolean) => void;
  onRun: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const last = s.runs[0];
  return (
    <>
      <tr className={`admin-session schedule-row${s.enabled ? "" : " disabled"}`} data-id={s.id} onClick={onOpen}>
        <td>
          <label className="check schedule-name" onClick={(e) => e.stopPropagation()}>
            <input type="checkbox" checked={s.enabled} onChange={(e) => onToggle(e.target.checked)} aria-label={t("Enabled")} />
            <strong>{s.name}</strong>
          </label>
          <div className="muted small">
            {t(KINDS[s.task.kind])} · {connection}
          </div>
        </td>
        <td className="small">{describeWhen(s)}</td>
        <td className="small" data-field="next">
          {s.running ? (
            <span className="admin-badge running">
              <span className="admin-dot" />
              {t("running")}
            </span>
          ) : s.next ? (
            when(s.next)
          ) : (
            <span className="muted">{t("off")}</span>
          )}
        </td>
        <td className="small" data-field="last">
          {last ? (
            <RunBadge ok={last.ok} title={last.message}>
              {" · "}
              {when(last.started)}
            </RunBadge>
          ) : (
            <span className="muted">{t("never")}</span>
          )}
        </td>
        <td className="schedule-actions" onClick={(e) => e.stopPropagation()}>
          <button type="button" disabled={s.running} onClick={onRun}>
            {t("Run now")}
          </button>
          <button type="button" onClick={onEdit}>
            {t("Edit…")}
          </button>
          <button type="button" className="admin-end" onClick={onDelete}>
            {t("Delete")}
          </button>
        </td>
      </tr>
      {opened && (
        <tr className="schedule-runs">
          <td colSpan={5}>
            {s.runs.length === 0 ? (
              <p className="muted small">{t("It has not run yet.")}</p>
            ) : (
              <ul>
                {s.runs.map((r, i) => (
                  <li key={i} className={r.ok ? "ok" : "failed"}>
                    <RunBadge ok={r.ok} />
                    <span className="muted">{when(r.started)}</span>
                    {r.manual && <span className="muted">{t("by hand")}</span>}
                    {r.attempt > 1 && <span className="muted">{t("try {n}", { n: r.attempt })}</span>}
                    <span className="schedule-message">{r.message}</span>
                    {r.file && <code className="schedule-file">{r.file}</code>}
                  </li>
                ))}
              </ul>
            )}
          </td>
        </tr>
      )}
    </>
  );
}

function ScheduleForm({
  initial,
  onSaved,
  onCancel,
}: {
  initial: Schedule;
  onSaved: (all: ScheduleView[]) => void;
  onCancel: () => void;
}) {
  const [s, setS] = useState<Schedule>(initial);
  const [zones, setZones] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const connections = workbench().connections();
  const connection = connections.find((c) => c.id === s.connection_id);
  // A connection that keeps its data in runs scripts only.
  const kinds = !connection ? (["script"] as const) : keepsDataIn(connection) ? (["script"] as const) : taskKinds(connection.driver);
  const set = (patch: Partial<Schedule>) => setS((old) => ({ ...old, ...patch }));
  const setTask = (task: ScheduleTask) => set({ task });
  const days = useMemo(weekdays, []);

  useEffect(() => {
    api.timeZones().then(setZones, () => setZones([]));
  }, []);

  async function choose(what: "script" | "folder") {
    const path = await answerFor(what === "script" ? "file" : "folder", () =>
      what === "script"
        ? open({ multiple: false, filters: [{ name: "SQL", extensions: ["sql"] }] })
        : open({ directory: true, multiple: false }),
    );
    if (typeof path !== "string") return;
    if (what === "folder") set({ folder: path });
    else if (s.task.kind === "script") setTask({ ...s.task, path });
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      onSaved(await api.saveSchedule(cleaned(s)));
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <form
      className="schedule-form"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <h2>{initial.name ? t("Edit {name}", { name: initial.name }) : t("New scheduled task")}</h2>
      <div className="field-grid">
        <label className="field">
          <span>{t("Name")}</span>
          <input autoFocus value={s.name} data-field="name" onChange={(e) => set({ name: e.target.value })} />
        </label>
        <label className="field">
          <span>{t("Connection")}</span>
          <select
            value={s.connection_id}
            data-field="connection"
            onChange={(e) => {
              const next = connections.find((c) => c.id === e.target.value);
              set({
                connection_id: e.target.value,
                task: !next ? s.task : keepsDataIn(next) ? taskOf("script", next.driver) : taskOnConnection(s.task, connection?.driver, next.driver),
              });
            }}
          >
            {connections.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label className="field">
        <span>{t("Task")}</span>
        <select
          value={s.task.kind}
          data-field="kind"
          onChange={(e) => setTask(taskOf(e.target.value as ScheduleTask["kind"], connection?.driver ?? "sqlite"))}
        >
          {kinds.map((k) => (
            <option key={k} value={k}>
              {t(KINDS[k])}
            </option>
          ))}
        </select>
      </label>
      {s.task.kind === "script" && (
        <div className="field">
          <span>{t("SQL file")}</span>
          <div className="row tight">
            <input
              value={s.task.path}
              data-field="script"
              spellCheck={false}
              onChange={(e) => s.task.kind === "script" && setTask({ ...s.task, path: e.target.value })}
            />
            <button type="button" onClick={() => void choose("script")}>
              {t("Choose file…")}
            </button>
          </div>
        </div>
      )}
      {s.task.kind === "export" && (
        <div className="field-grid schedule-export">
          <label className="field">
            <span>{t("Query")}</span>
            <textarea
              rows={4}
              value={s.task.sql}
              data-field="sql"
              spellCheck={false}
              onChange={(e) => s.task.kind === "export" && setTask({ ...s.task, sql: e.target.value })}
            />
          </label>
          <label className="field">
            <span>{t("Format")}</span>
            <select
              value={s.task.format}
              data-field="format"
              onChange={(e) =>
                s.task.kind === "export" &&
                setTask({ ...s.task, format: e.target.value as ScheduleExportFormat })
              }
            >
              <option value="csv">CSV</option>
              <option value="json">JSON</option>
              <option value="parquet">Parquet</option>
              <option value="xlsx">Excel</option>
            </select>
          </label>
        </div>
      )}
      {s.task.kind === "backup" && connection && connection.driver !== "mysql" && (
        <div className="field-grid">
          {connection.driver === "postgres" ? (
            <label className="field">
              <span>{t("Format")}</span>
              <select
                value={s.task.format}
                data-field="format"
                onChange={(e) => s.task.kind === "backup" && setTask({ ...s.task, format: e.target.value as BackupFormat })}
              >
                <option value="custom">{t("Archive for pg_restore (.dump)")}</option>
                <option value="plain">{t("SQL statements (.sql)")}</option>
              </select>
            </label>
          ) : (
            <p className="muted small">{t("Written as a SQL dump of the schema, which Run SQL file runs again.")}</p>
          )}
          <label className="field">
            <span>{t(connection.driver === "postgres" ? "Only the schema" : "Schema")}</span>
            <input
              value={s.task.schema ?? ""}
              placeholder={t(connection.driver === "postgres" ? "The whole database" : "The connection's own")}
              data-field="schema"
              onChange={(e) => s.task.kind === "backup" && setTask({ ...s.task, schema: e.target.value || null })}
            />
          </label>
        </div>
      )}
      <div className="field-grid schedule-when">
        <label className="field">
          <span>{t("Runs")}</span>
          <select
            value={s.repeat.every}
            data-field="every"
            onChange={(e) => set({ repeat: repeatOf(e.target.value as ScheduleRepeat["every"], s.repeat) })}
          >
            {EVERY.map(([value, text]) => (
              <option key={value} value={value}>
                {t(text)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{s.repeat.every === "hour" ? t("At the minute of") : t("At")}</span>
          <input type="time" value={s.time} data-field="time" onChange={(e) => set({ time: e.target.value })} />
        </label>
        <label className="field">
          <span>{t("Time zone")}</span>
          <select value={s.timezone} data-field="timezone" onChange={(e) => set({ timezone: e.target.value })}>
            <option value="">{t("The system's")}</option>
            {zones.map((z) => (
              <option key={z} value={z}>
                {z}
              </option>
            ))}
          </select>
        </label>
      </div>
      {s.repeat.every === "week" && (
        <fieldset className="choices schedule-days" data-field="days">
          {days.map((name, i) => {
            const day = i + 1;
            const on = s.repeat.every === "week" && s.repeat.days.includes(day);
            return (
              <label className="check" key={day}>
                <input
                  type="checkbox"
                  checked={on}
                  onChange={(e) =>
                    s.repeat.every === "week" &&
                    set({
                      repeat: {
                        every: "week",
                        days: e.target.checked ? [...s.repeat.days, day].sort() : s.repeat.days.filter((d) => d !== day),
                      },
                    })
                  }
                />
                {name}
              </label>
            );
          })}
        </fieldset>
      )}
      {s.repeat.every === "month" && (
        <label className="field schedule-day">
          <span>{t("Day of the month")}</span>
          <input
            type="number"
            min={1}
            max={31}
            value={s.repeat.day}
            data-field="day"
            onChange={(e) => set({ repeat: { every: "month", day: Number(e.target.value) } })}
          />
        </label>
      )}
      {writesFiles(s.task) && (
        <div className="field-grid">
          <div className="field">
            <span>{t("Folder")}</span>
            <div className="row tight">
              <input
                value={s.folder ?? ""}
                data-field="folder"
                spellCheck={false}
                onChange={(e) => set({ folder: e.target.value })}
              />
              <button type="button" onClick={() => void choose("folder")}>
                {t("Choose folder…")}
              </button>
            </div>
          </div>
          <label className="field">
            <span>{t("Files kept")}</span>
            <input
              type="number"
              min={0}
              value={s.keep}
              data-field="keep"
              title={t("The newest files of the task stay in the folder; 0 keeps them all.")}
              onChange={(e) => set({ keep: Math.max(0, Number(e.target.value) || 0) })}
            />
          </label>
        </div>
      )}
      <div className="field-grid">
        <label className="field">
          <span>{t("Tries again if it fails")}</span>
          <input
            type="number"
            min={0}
            max={10}
            value={s.retries}
            data-field="retries"
            onChange={(e) => set({ retries: Math.max(0, Number(e.target.value) || 0) })}
          />
        </label>
        <label className="field">
          <span>{t("Minutes between tries")}</span>
          <input
            type="number"
            min={1}
            value={s.retry_minutes}
            data-field="retry_minutes"
            disabled={s.retries === 0}
            onChange={(e) => set({ retry_minutes: Math.max(1, Number(e.target.value) || 1) })}
          />
        </label>
      </div>
      <label className="check">
        <input type="checkbox" checked={s.enabled} data-field="enabled" onChange={(e) => set({ enabled: e.target.checked })} />
        {t("Enabled")}
      </label>
      <p className="muted small">{t("A task runs with the connection's saved password: it cannot ask for one.")}</p>
      {error && (
        <div className="notice error small" data-field="error">
          {error}
        </div>
      )}
      <div className="actions">
        <span className="spacer" />
        <button type="button" onClick={onCancel}>
          {t("Cancel")}
        </button>
        <button type="submit" className="primary" disabled={saving}>
          {t("Save")}
        </button>
      </div>
    </form>
  );
}
