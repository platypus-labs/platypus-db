/** "Query with SQL": the rows of a tab's results, loaded in full up to the memory limit,
 *  become tables of an in-memory DuckDB, queried in a new SQL editor on a temporary
 *  connection. The tab is a copy: "Reload from result" copies the rows again. */
import { useEffect, useRef, useState } from "react";
import { registerAction } from "../actions";
import { api, isMongo, type ResultTable, type SavedConnection, toCommandError } from "../api";
import { fmt } from "../components/ResultPanel";
import { msg, t } from "../i18n";
import { workbench } from "../workbench";
import { type ResultHost, type ResultSource, hostOfJob, hostOfTab, initialSql, resultTables, shownHost } from "./resultHosts";

/** Loads every row of the results, showing how many with Cancel; false when cancelled or
 *  failed. */
function loadAll(jobIds: number[]): Promise<boolean> {
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
  const cancelled = useRef(false);
  useEffect(() => {
    const counts = new Map<number, number>();
    const progress = (id: number) => (n: number) => {
      counts.set(id, n);
      setRows([...counts.values()].reduce((sum, v) => sum + v, 0));
    };
    Promise.all(jobIds.map((id) => api.fetchWhole(id, progress(id)))).then(
      () => onDone(!cancelled.current),
      (e: unknown) => {
        workbench().showError(toCommandError(e).message);
        onDone(false);
      },
    );
  }, []);
  const cancel = () => {
    cancelled.current = true;
    for (const id of jobIds) void api.stopFetch(id).catch(() => undefined);
  };
  return (
    <div className="modal-backdrop">
      <div className="modal small load-rows" aria-modal="true">
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

/** Opens a SQL editor on the results of `host`: `jobId` (else the shown one) is `result`. */
export async function queryWithSql(host: ResultHost, jobId?: number) {
  const tables = resultTables(host.results(), jobId ?? host.activeJob());
  if (tables.length === 0) return workbench().showError(t("This tab has no result with rows."));
  if (!(await loadAll(tables.map((x) => x.job_id)))) return;
  const opened = await api.openResultDb(t("Results of {name}", { name: host.connection.name }), tables);
  const source: ResultSource = { tabKey: host.tabKey, tables: tableNames(tables), partial: opened.partial };
  workbench().openTemporaryTab(
    "query",
    opened.connection,
    { sql: initialSql(tables), title: t("SQL on results"), resultSource: source },
    () => void api.closeResultDb(opened.connection.id),
  );
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
  const [busy, setBusy] = useState(false);
  const go = async () => {
    setBusy(true);
    try {
      const next = await reload(source, connection);
      if (next) onReloaded(next);
    } catch (e) {
      workbench().showError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className={`notice ${source.partial ? "warn" : "muted"} result-source`}>
      <span>{t("Tables of the results of another tab: {tables}", { tables: source.tables.join(", ") })}</span>
      {source.partial && <strong>{t("Partial: reading stopped at the memory limit.")}</strong>}
      <button type="button" disabled={busy} onClick={() => void go()}>
        {t("Reload from result")}
      </button>
    </div>
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
  run: () => {
    const host = shownHost();
    return host ? queryWithSql(host) : undefined;
  },
});

registerAction({
  id: "grid.query_with_sql",
  scope: "grid",
  title: "Query with SQL",
  group: "analysis",
  order: 70,
  when: ({ source }) => source?.jobId != null && !isMongo(source.connection.driver) && hostOfJob(source.jobId) !== null,
  run: ({ source }) => {
    const jobId = source?.jobId;
    const host = jobId == null ? null : hostOfJob(jobId);
    return host && jobId != null ? queryWithSql(host, jobId) : undefined;
  },
});
