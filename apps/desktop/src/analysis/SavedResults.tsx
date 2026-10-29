/** Saved results: "Save result…" keeps the rows of a result in a file of the data folder; the
 *  panel under the scripts lists them. Opening one reads the file into an in-memory DuckDB (the
 *  mechanism of "Query with SQL") and runs `SELECT * FROM result` in a tab on it, so the grid,
 *  charts, statistics and SQL on results work with no connection to the original database. */
import { useCallback, useEffect, useState } from "react";
import { registerAction } from "../actions";
import { api, type SavedResult, toCommandError } from "../api";
import { fmt } from "../components/ResultPanel";
import { locale, plural, t } from "../i18n";
import { withDriver } from "../drivers/DriverPopup";
import { Icon } from "../ui/Icon";
import { ask } from "../ui/ask";
import { askName } from "../ui/NameDialog";
import { workbench } from "../workbench";
import { loadAll, openTemporaryQuery } from "./QueryWithSql";
import { type ResultHost, hostOfJob } from "./resultHosts";
import { defaultName, filterSaved } from "./savedResultsModel";

const listeners = new Set<() => void>();

/** The saved results changed: every panel reads them again. */
function savedResultsChanged() {
  for (const l of listeners) l();
}

/** When a result was saved, as the interface's language writes it. */
const savedTime = (iso: string) => new Date(iso).toLocaleString(locale(), { dateStyle: "medium", timeStyle: "short" });

/** Saves the rows of result `jobId` of `host`, after asking a name: the rows are loaded in
 *  full first, up to the memory limit (the file is marked partial when they stop there). */
export async function saveResult(host: ResultHost, jobId: number, sql: string) {
  if (!(await loadAll([jobId]))) return;
  const initial = defaultName(sql, t("Result"));
  askName(t("Save result"), initial, async (name) => {
    const saved = await api.saveResult(jobId, {
      name,
      sql,
      connection: host.connection.name,
      driver: host.connection.driver,
      database: host.database() ?? (host.connection.database || null),
    });
    savedResultsChanged();
    if (saved.partial) workbench().showError(t("Saved, but only the rows loaded: reading stopped before the end of the result."));
  });
}

/** Opens a saved result in a SQL tab on a temporary in-memory DuckDB. */
export async function openSavedResult(saved: SavedResult) {
  try {
    // DuckDB holds the rows: downloaded first if it is not yet.
    const opened = await withDriver(() => api.openSavedResult(saved.id, t("Saved result: {name}", { name: saved.name })));
    openTemporaryQuery(opened.connection, { sql: "SELECT * FROM result", title: saved.name, autorun: true, savedResult: opened.result });
  } catch (e) {
    workbench().showError(toCommandError(e).message);
  }
}

registerAction({
  id: "grid.save_result",
  scope: "grid",
  title: "Save result…",
  group: "analysis",
  order: 75,
  when: ({ source }) => source?.jobId != null && hostOfJob(source.jobId) !== null,
  run: ({ source }) => {
    const host = source?.jobId == null ? null : hostOfJob(source.jobId);
    if (host && source?.jobId != null) return saveResult(host, source.jobId, source.sql);
  },
});

/** The saved results, under the scripts: search, open, rename and delete. */
export function SavedResultsPanel({ onError }: { onError: (message: string) => void }) {
  const [list, setList] = useState<SavedResult[] | null>(null);
  const [open, setOpen] = useState(true);
  const [search, setSearch] = useState("");
  const fail = useCallback((e: unknown) => onError(toCommandError(e).message), [onError]);
  const load = useCallback(() => {
    api.listSavedResults().then(setList, fail);
  }, [fail]);
  useEffect(load, [load]);
  useEffect(() => {
    listeners.add(load);
    return () => void listeners.delete(load);
  }, [load]);

  const rename = (saved: SavedResult) =>
    askName(t("Rename saved result"), saved.name, async (name) => {
      await api.renameSavedResult(saved.id, name);
      savedResultsChanged();
    });
  const remove = async (saved: SavedResult) => {
    if (!(await ask(t("Delete the saved result “{name}”? Its file is deleted too.", { name: saved.name }), { confirm: t("Delete"), danger: true }))) return;
    try {
      await api.deleteSavedResult(saved.id);
      savedResultsChanged();
    } catch (e) {
      fail(e);
    }
  };

  const shown = filterSaved(list ?? [], search);
  return (
    <section className={`scripts-panel saved-results-panel${open ? " open" : ""}`} aria-label={t("Saved results")}>
      <div className="sidebar-header scripts-header">
        <button
          className="icon twisty-button"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          title={t(open ? "Hide saved results" : "Show saved results")}
        >
          <Icon name={open ? "chevron-down" : "chevron-right"} size={12} />
        </button>
        <span className="sidebar-title">{t("Saved results")}</span>
      </div>
      {open && (list?.length ?? 0) > 0 && (
        <input
          className="saved-results-search"
          type="search"
          value={search}
          placeholder={t("Search saved results")}
          aria-label={t("Search saved results")}
          onChange={(e) => setSearch(e.target.value)}
        />
      )}
      {open && (
        <ul className="tree scripts-tree">
          {shown.map((saved) => (
            <li
              key={saved.id}
              className="tree-row saved-result-row"
              data-name={saved.name}
              tabIndex={0}
              title={saved.sql}
              onDoubleClick={() => void openSavedResult(saved)}
              onKeyDown={(e) => e.key === "Enter" && void openSavedResult(saved)}
            >
              <span className="tree-icon">
                <Icon name="table" />
              </span>
              <span className="tree-label">{saved.name}</span>
              <span className="tree-detail" title={savedTime(saved.saved_at)}>
                {saved.connection} · {savedTime(saved.saved_at)} ·{" "}
                {plural(saved.rows, "{n} row", "{n} rows")}
                {saved.partial ? ` · ${t("partial")}` : ""}
              </span>
              <button className="icon" title={t("Open")} aria-label={t("Open")} onClick={() => void openSavedResult(saved)}>
                <Icon name="open" />
              </button>
              <button className="icon" title={t("Rename")} aria-label={t("Rename")} onClick={() => rename(saved)}>
                <Icon name="edit" />
              </button>
              <button className="icon" title={t("Delete")} aria-label={t("Delete")} onClick={() => void remove(saved)}>
                <Icon name="delete" />
              </button>
            </li>
          ))}
          {list && list.length === 0 && <li className="muted small scripts-empty">{t("No saved results")}</li>}
          {list && list.length > 0 && shown.length === 0 && (
            <li className="muted small scripts-empty">{t("No saved result matches")}</li>
          )}
        </ul>
      )}
    </section>
  );
}

/** Above the results of a saved result's tab: where the rows come from. */
export function SavedResultBar({ saved }: { saved: SavedResult }) {
  return (
    <div className={`notice ${saved.partial ? "warn" : "muted"} result-source`}>
      <span>
        {t("Saved result of {connection} from {time}: {rows} rows. No connection is needed to read it.", {
          connection: saved.connection,
          time: savedTime(saved.saved_at),
          rows: fmt.format(saved.rows),
        })}
      </span>
      {saved.partial && <strong>{t("Partial: reading stopped before the end of the rows.")}</strong>}
    </div>
  );
}
