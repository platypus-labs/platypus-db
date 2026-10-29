/** A maintenance operation on a table, from the navigator: what it does, then once run, what
 *  the server said of it. Runs on the navigator's session, in autocommit: `VACUUM` cannot
 *  run in a transaction. */
import { useState } from "react";
import { api, type Maintenance, type SavedConnection, toCommandError } from "../api";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { MAINTENANCE } from "./Statistics";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  table: string;
  operation: Maintenance;
  onClose: () => void;
}

type State = { step: "ask" } | { step: "running" } | { step: "done"; lines: string[] } | { step: "failed"; message: string };

export function MaintenanceDialog({ connection, database, schema, table, operation, onClose }: Props) {
  const [state, setState] = useState<State>({ step: "ask" });
  const { label, what } = MAINTENANCE[operation];

  async function run() {
    setState({ step: "running" });
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return onClose();
      setState({ step: "done", lines: await api.maintainTable(sid, schema, table, operation) });
    } catch (e) {
      setState({ step: "failed", message: toCommandError(e).message });
    }
  }

  const close = () => state.step !== "running" && onClose();
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal maintenance-dialog" role="dialog" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{t("{operation} {table}", { operation: t(label), table: `${schema}.${table}` })}</h2>
        <p className="muted small">{t(what)}</p>
        {state.step === "done" && (
          <>
            <div className="notice ok small" data-field="maintenance-result">
              {t("{operation} done.", { operation: t(label) })}
            </div>
            {state.lines.length > 0 && <pre className="admin-statement admin-maintenance-output">{state.lines.join("\n")}</pre>}
          </>
        )}
        {state.step === "failed" && (
          <div className="notice error small" data-field="maintenance-result">
            {state.message}
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          {state.step === "ask" || state.step === "running" ? (
            <>
              <button type="button" disabled={state.step === "running"} onClick={onClose}>
                {t("Cancel")}
              </button>
              <button type="button" className="primary" autoFocus disabled={state.step === "running"} onClick={() => void run()}>
                {state.step === "running" ? t("Running…") : t(label)}
              </button>
            </>
          ) : (
            <button type="button" className="primary" autoFocus onClick={onClose}>
              {t("Close")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
