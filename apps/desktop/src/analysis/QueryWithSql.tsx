/** "Query with SQL": the rows of a tab's results, loaded in full up to the memory limit,
 *  become tables of an in-memory DuckDB. The field above a result's grid queries them in place
 *  (`ResultSql.tsx`); "Open in a tab" queries them in a new SQL editor on a temporary
 *  connection, a copy that "Reload from result" copies again. */
import { type ReactNode, useEffect, useRef, useState } from "react";
import { registerAction } from "../actions";
import { api, isMongo, type ResultTable, type SavedConnection, toCommandError } from "../api";
import { fmt } from "../components/ResultPanel";
import { msg, t } from "../i18n";
import { workbench } from "../workbench";
import { withDriver } from "../drivers/DriverPopup";
import type { TabInit } from "../tabs/registry";
import { type ResultHost, type ResultSource, hostOfJob, hostOfTab, initialSql, resultTables, shownHost } from "./resultHosts";

/** Loads every row of the results, showing how many with Cancel; false when cancelled or
 *  failed (a result closed meanwhile fails). Cancel closes the dialog at once. */
export function loadAll(jobIds: number[]): Promise<boolean> {
  return new Promise((resolve) =>
    workbench().openDialog((close) => (
      <LoadDialog
        jobIds={[...new Set(jobIds)]}
        onDone={(ok) => {
          close();
          resolve(ok);
        }}
      />
    )),
  );
}

function LoadDialog({ jobIds, onDone }: { jobIds: number[]; onDone: (ok: boolean) => void }) {
  const [rows, setRows] = useState(0);
  const finished = useRef(false);
  const finish = (ok: boolean) => {
    if (finished.current) return;
    finished.current = true;
    onDone(ok);
  };
  useEffect(() => {
    const counts = new Map<number, number>();
    const progress = (id: number) => (n: number) => {
      counts.set(id, n);
      setRows([...counts.values()].reduce((sum, v) => sum + v, 0));
    };
    Promise.all(jobIds.map((id) => api.fetchWhole(id, progress(id)))).then(
      () => finish(true),
      (e: unknown) => {
        if (!finished.current) workbench().showError(toCommandError(e).message);
        finish(false);
      },
    );
  }, []);
  const cancel = () => {
    finish(false);
    // The fetches end with the jobs; a result closed meanwhile has nothing to stop.
    for (const id of jobIds) void api.stopFetch(id).catch(() => undefined);
  };
  return (
    <div className="modal-backdrop">
      <div className="modal small load-rows" role="dialog" aria-modal="true" aria-label={t("Query with SQL")}>
        <h2>{t("Query with SQL")}</h2>
        <p>{t("Loading all the rows of the result: {rows}", { rows: fmt.format(rows) })}</p>
        <div className="result-loading" role="progressbar" aria-label={t("Loading rows")} />
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={cancel}>
            {t("Cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}

const tableNames = (tables: ResultTable[]) => [...new Set(tables.map((x) => x.table))];

/** Opens a SQL editor tab on the temporary DuckDB connection `connection`; closing the tab
 *  forgets the database. */
export function openTemporaryQuery(connection: SavedConnection, init: TabInit<"query">) {
  workbench().openTemporaryTab("query", connection, init, () => void api.closeResultDb(connection.id));
}

/** Opens a SQL editor on the results of `host`: `jobId` (else the shown one) is `result`;
 *  `sql` is the editor's text, else a query of `result`. */
export async function queryWithSql(host: ResultHost, jobId?: number, sql?: string) {
  const tables = resultTables(host.results(), jobId ?? host.activeJob());
  if (tables.length === 0) return workbench().showError(t("This tab has no result with rows."));
  if (!(await loadAll(tables.map((x) => x.job_id)))) return;
  // DuckDB holds the results: downloaded first if it is not yet.
  const opened = await withDriver(() => api.openResultDb(t("Results of {name}", { name: host.connection.name }), tables));
  const source: ResultSource = { tabKey: host.tabKey, tables: tableNames(tables), partial: opened.partial };
  openTemporaryQuery(opened.connection, { sql: sql ?? initialSql(tables), title: t("SQL on results"), resultSource: source });
}

/** Copies the source tab's results again into the tables of `connection`; `null` when
 *  cancelled. */
async function reload(source: ResultSource, connection: SavedConnection): Promise<ResultSource | null> {
  const host = hostOfTab(source.tabKey);
  if (!host) {
    workbench().showError(t("The tab the results came from is closed."));
    return null;
  }
  const tables = resultTables(host.results(), host.activeJob());
  if (tables.length === 0) {
    workbench().showError(t("This tab has no result with rows."));
    return null;
  }
  if (!(await loadAll(tables.map((x) => x.job_id)))) return null;
  const partial = await api.reloadResultDb(connection.id, tables);
  return { ...source, tables: tableNames(tables), partial };
}

/** A bar above the results of a tab on a temporary connection: `children` tell where its
 *  tables come from, and the button copies them again with `reload` (`null` when cancelled). */
export function ReloadBar<T>({
  warn,
  className,
  label,
  action,
  reload,
  onReloaded,
  children,
}: {
  warn: boolean;
  className?: string;
  label: string;
  action?: string;
  reload: () => Promise<T | null>;
  onReloaded: (next: T) => void;
  children: ReactNode;
}) {
  const [busy, setBusy] = useState(false);
  const go = async () => {
    setBusy(true);
    try {
      const next = await reload();
      if (next) onReloaded(next);
    } catch (e) {
      workbench().showError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className={`notice ${warn ? "warn" : "muted"} result-source${className ? ` ${className}` : ""}`}>
      {children}
      <button type="button" data-action={action} disabled={busy} onClick={() => void go()}>
        {label}
      </button>
    </div>
  );
}

/** Above the results of a "Query with SQL" tab: its tables, a warning when they lack rows,
 *  and Reload from result. */
export function ResultSourceBar({
  source,
  connection,
  onReloaded,
}: {
  source: ResultSource;
  connection: SavedConnection;
  onReloaded: (source: ResultSource) => void;
}) {
  return (
    <ReloadBar warn={source.partial !== null} label={t("Reload from result")} reload={() => reload(source, connection)} onReloaded={onReloaded}>
      <span>{t("Tables of the results of another tab: {tables}", { tables: source.tables.join(", ") })}</span>
      {source.partial && (
        <strong>
          {source.partial === "memory_limit"
            ? t("Partial: reading stopped at the memory limit.")
            : t("Partial: reading stopped before the end of the rows.")}
        </strong>
      )}
    </ReloadBar>
  );
}

registerAction({
  id: "analysis.query_with_sql",
  scope: "global",
  title: "Query the result with SQL",
  disabled: () => {
    const host = shownHost();
    if (!host || host.results().length === 0) return msg("No result with rows in the tab shown");
    return isMongo(host.connection.driver) ? msg("MongoDB results cannot be queried with SQL") : null;
  },
  run: () => shownHost()?.querySql(),
});

registerAction({
  id: "grid.query_with_sql",
  scope: "grid",
  title: "Query with SQL",
  group: "analysis",
  order: 70,
  when: ({ source }) => source?.jobId != null && !isMongo(source.connection.driver) && hostOfJob(source.jobId) !== null,
  run: ({ source }) => (source?.jobId == null ? undefined : hostOfJob(source.jobId)?.querySql()),
});

registerAction({
  id: "analysis.pivot",
  scope: "global",
  title: "Pivot the result",
  disabled: () => {
    const host = shownHost();
    if (!host || host.results().length === 0) return msg("No result with rows in the tab shown");
    return isMongo(host.connection.driver) ? msg("MongoDB results cannot be queried with SQL") : null;
  },
  run: () => shownHost()?.pivot(),
});

registerAction({
  id: "grid.pivot",
  scope: "grid",
  title: "Pivot…",
  group: "analysis",
  order: 71,
  when: ({ source }) => source?.jobId != null && !isMongo(source.connection.driver) && hostOfJob(source.jobId) !== null,
  run: ({ source }) => (source?.jobId == null ? undefined : hostOfJob(source.jobId)?.pivot()),
});
