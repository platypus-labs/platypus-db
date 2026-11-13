import { createElement, useEffect, useState } from "react";
import { SchedulesDialog } from "../admin/SchedulesDialog";
import { api, type SchemaChange, type SchemaDiff, type SchemaSnapshot, toCommandError } from "../api";
import { locale, msg, plural, t } from "../i18n";
import type { TabBase, TabProps } from "../tabs/registry";
import { ask } from "../ui/ask";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { NOW, type Pick, groupChanges, pair, stillThere, toggled } from "./history";
import { commentHeader } from "./script";
import { translateWarning } from "./warnings";
import { EmptyState } from "../ui/State";

/** The history of a schema's structure: the snapshots taken, by hand or by a scheduled task,
 *  and what changed between two of them, or between one and the schema now. */
interface HistoryTabModel extends TabBase {
  kind: "schema-history";
  schema: string;
}

declare module "../tabs/registry" {
  interface TabKinds {
    "schema-history": HistoryTabModel;
  }
}

const HOW: Record<SchemaSnapshot["how"], string> = {
  manual: msg("Manual"),
  scheduled: msg("Scheduled"),
};

/** The names of what a column change is about, as `platypus-schema` words them. */
const ATTRIBUTES: Record<string, string> = {
  "primary key": msg("Primary key"),
  comment: msg("Comment"),
  extra: msg("Extra"),
  generated: msg("Generated"),
  collation: msg("Collation"),
};

const when = (iso: string) => new Date(iso).toLocaleString(locale(), { dateStyle: "medium", timeStyle: "medium" });

/** A change in words; the table is the heading above it. */
function describe(c: SchemaChange): string {
  const name = c.item ?? "";
  const none = (v: string | null) => v ?? t("none");
  switch (c.kind) {
    case "table_added":
      return t("Table added");
    case "table_dropped":
      return t("Table dropped");
    case "column_added":
      return t("Column {name} added ({type})", { name, type: c.to ?? "" });
    case "column_dropped":
      return t("Column {name} dropped", { name });
    case "column_type_changed":
      return t("Column {name}: type {from} → {to}", { name, from: none(c.from), to: none(c.to) });
    case "column_nullability_changed":
      return t("Column {name}: {from} → {to}", { name, from: none(c.from), to: none(c.to) });
    case "column_default_changed":
      return t("Column {name}: default {from} → {to}", { name, from: none(c.from), to: none(c.to) });
    case "column_changed": {
      const what = (c.to ?? "").split(", ").map((a) => t(ATTRIBUTES[a] ?? a));
      return t("Column {name}: {what} changed", { name, what: what.join(", ") });
    }
    case "index_added":
      return t("Index {name} added", { name });
    case "index_dropped":
      return t("Index {name} dropped", { name });
    case "foreign_key_added":
      return t("Foreign key {name} added", { name });
    case "foreign_key_dropped":
      return t("Foreign key {name} dropped", { name });
    case "constraint_added":
      return t("Constraint {name} added", { name });
    case "constraint_dropped":
      return t("Constraint {name} dropped", { name });
    case "comment_changed":
      return t("Table comment changed");
  }
}

export function HistoryTab({ tab, connection, active, sideSession }: TabProps<HistoryTabModel>) {
  const database = tab.database ?? null;
  const [snapshots, setSnapshots] = useState<SchemaSnapshot[] | null>(null);
  const [picks, setPicks] = useState<Pick[]>([]);
  const [diff, setDiff] = useState<SchemaDiff | null>(null);
  const [comparing, setComparing] = useState(false);
  const [label, setLabel] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Counts the refreshes, which compare the schema now again.
  const [version, setVersion] = useState(0);
  const compared = pair(picks);

  const load = () =>
    api.listSchemaSnapshots(connection.id, database, tab.schema).then(
      (all) => {
        setSnapshots(all);
        setPicks((old) => stillThere(old, all));
      },
      (e) => setError(toCommandError(e).message),
    );

  useEffect(() => {
    void load();
  }, [connection.id, database, tab.schema]);

  // The changes between the two states picked.
  const [from, to] = compared ?? [null, null];
  useEffect(() => {
    setDiff(null);
    if (from === null || to === null) return;
    let live = true;
    setComparing(true);
    setError(null);
    void (async () => {
      try {
        const session = to === NOW ? await sideSession() : 0;
        if (session === null) return;
        const result = await api.diffSchemaSnapshots(from, to === NOW ? { kind: "live", session_id: session } : { kind: "snapshot", id: to });
        if (live) setDiff(result);
      } catch (e) {
        if (live) setError(toCommandError(e).message);
      } finally {
        if (live) setComparing(false);
      }
    })();
    return () => {
      live = false;
    };
  }, [from, to, version]);

  async function take() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const session = await sideSession();
      if (session === null) return;
      const taken = await api.takeSchemaSnapshot(session, connection.id, database, tab.schema, label.trim() || null);
      setNotice(taken.unchanged ? t("No changes since the last snapshot") : t("Snapshot taken"));
      if (!taken.unchanged) setLabel("");
      await load();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function remove(s: SchemaSnapshot) {
    if (!(await ask(t("Delete the snapshot of {time}?", { time: when(s.taken_at) }), { confirm: t("Delete"), danger: true }))) return;
    try {
      await api.deleteSchemaSnapshot(s.id);
      setNotice(null);
      await load();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const schedule = () =>
    workbench().openDialog((close) => createElement(SchedulesDialog, { connectionId: connection.id, snapshot: { database, schema: tab.schema }, onClose: close }));

  /** The script of the changes in the connection's editor, headed by what it turns into what. */
  function openScript() {
    if (!diff || from === null || to === null) return;
    const state = (p: Pick) => (p === NOW ? t("Now") : when(snapshots?.find((s) => s.id === p)?.taken_at ?? ""));
    const head = commentHeader(
      t("Turns {schema} as of {from} into {to}", { schema: tab.schema, from: state(from), to: state(to) }),
      diff.warnings.map(translateWarning),
    );
    workbench().openTab("query", connection.id, { sql: `${head}${diff.sql}`, database });
  }

  const groups = groupChanges(diff?.changes ?? []);
  return (
    <div className="object-tab schema-history" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon" data-kind="schema">
          <Icon name="schema" size={16} />
        </span>
        <span>
          <strong>{t("History of {schema}", { schema: tab.schema })}</strong>{" "}
          <span className="muted small">{snapshots ? plural(snapshots.length, "{n} snapshot", "{n} snapshots") : ""}</span>
        </span>
        <span className="spacer" />
        <input
          className="history-label"
          value={label}
          placeholder={t("Label (optional)")}
          aria-label={t("Label (optional)")}
          disabled={busy}
          onChange={(e) => setLabel(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !busy && void take()}
        />
        <button type="button" className="primary" disabled={busy} onClick={() => void take()}>
          {t("Take snapshot")}
        </button>
        <button type="button" onClick={schedule}>
          {t("Schedule…")}
        </button>
        <button className="square" type="button" onClick={() => (setVersion((v) => v + 1), void load())} aria-label={t("Refresh")} title={t("Refresh")}>
          <Icon name="refresh" />
        </button>
      </header>
      {error && <div className="notice error small">{error}</div>}
      {notice && <div className="notice small history-notice">{notice}</div>}
      <div className="history-body">
        <table className="meta-table history-list">
          <thead>
            <tr>
              <th />
              <th>{t("When")}</th>
              <th>{t("Label")}</th>
              <th>{t("Type")}</th>
              <th>{t("Tables")}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            <tr className="history-row history-now" data-id="now">
              <td>
                <input type="checkbox" checked={picks.includes(NOW)} aria-label={t("Now")} onChange={() => setPicks(toggled(picks, NOW))} />
              </td>
              <td>
                <strong>{t("Now")}</strong>
              </td>
              <td className="muted" colSpan={4}>
                {t("The schema as it is in the database")}
              </td>
            </tr>
            {snapshots?.map((s) => (
              <tr key={s.id} className="history-row" data-id={s.id}>
                <td>
                  <input type="checkbox" checked={picks.includes(s.id)} aria-label={when(s.taken_at)} onChange={() => setPicks(toggled(picks, s.id))} />
                </td>
                <td>{when(s.taken_at)}</td>
                <td data-field="label">{s.label ?? ""}</td>
                <td data-field="how">{t(HOW[s.how])}</td>
                <td data-field="tables">{s.tables}</td>
                <td>
                  <button type="button" className="admin-end" onClick={() => void remove(s)}>
                    {t("Delete")}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {snapshots?.length === 0 && <EmptyState compact className="history-empty" title={t("No snapshots yet. Take one to start the history.")} />}
        <section className="history-changes">
          {compared === null ? (
            <p className="muted small history-hint">{t("Tick two snapshots, or one and Now, to see what changed between them.")}</p>
          ) : (
            <>
              <div className="row history-changes-head">
                <strong>{t("Changes")}</strong>
                {comparing && <span className="muted small">{t("Comparing…")}</span>}
                <span className="spacer" />
                <button type="button" disabled={!diff || diff.sql.trim() === ""} onClick={openScript}>
                  {t("Open script")}
                </button>
              </div>
              {diff && groups.length === 0 && <p className="muted small history-same">{t("No differences.")}</p>}
              {groups.map((g) => (
                <div key={g.table} className="history-table" data-table={g.table}>
                  <h3>{g.table}</h3>
                  <ul>
                    {g.changes.map((c, i) => (
                      <li key={i} className={`history-change ${c.kind}`}>
                        {describe(c)}
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </>
          )}
        </section>
      </div>
    </div>
  );
}
