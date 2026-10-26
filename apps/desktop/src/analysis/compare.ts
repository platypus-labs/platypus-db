/** "Compare with previous run" in a SQL editor tab: before the same statement runs again, the
 *  backend keeps the rows of the result it replaces; the new result is then compared with them
 *  in a diff tab. */
import { useRef } from "react";
import { api, isMongo, type SavedConnection } from "../api";
import type { Result } from "../components/ResultPanel";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { type ColumnLike, previousOf, sameColumns, sameStatement } from "./diffModel";

export function useCompare(connection: SavedConnection) {
  /** The statement and columns of the result the backend keeps for the tab; `null` for none. */
  const kept = useRef<{ sql: string; columns: ColumnLike[] } | null>(null);
  return {
    /** Before a run: keeps the result it replaces when it runs the same one statement again,
     *  else forgets the copy. The comparison is optional: a failure here never stops the run. */
    async keep(sessionId: number, planned: readonly { sql: string }[], results: readonly Result[]) {
      if (isMongo(connection.driver)) return;
      const old = planned.length === 1 ? previousOf(results, planned[0]!.sql) : null;
      try {
        if (old) {
          await api.keepPreviousRun(sessionId, old.jobId);
          kept.current = { sql: old.sql, columns: old.columns };
        } else if (kept.current) {
          kept.current = null;
          await api.forgetPreviousRun(sessionId);
        }
      } catch {
        // E.g. `not_ready`: the result has no rows yet, so nothing to keep. A copy of an
        // earlier run would compare with the wrong rows: it goes too.
        kept.current = null;
        await api.forgetPreviousRun(sessionId).catch(() => {});
      }
    },
    /** A copy of an earlier run of `result`'s statement is kept. */
    kept: (result: Result | undefined) =>
      !!result && kept.current !== null && result.status.kind === "rows" && sameStatement(kept.current.sql, result.sql),
    /** And it has the same columns: the two can be compared. */
    comparable: (result: Result | undefined) =>
      !!result?.columns && kept.current !== null && sameColumns(kept.current.columns, result.columns),
    /** Opens the differences from the kept run to result `jobId`, by `key` when known. */
    async open(sessionId: number, jobId: number, key: number[] | null, database: string | null) {
      const diff = await api.diffPrevious(sessionId, jobId, key);
      workbench().openTab("diff", connection.id, { database, title: t("Changes since the previous run"), diff });
    },
  };
}
