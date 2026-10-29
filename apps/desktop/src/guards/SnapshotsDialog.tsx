/** Snapshots before UPDATE and DELETE run from the editor: the setting, taking one before a
 *  statement, and the list with each snapshot's restore script. */
import { createElement, useEffect, useState } from "react";
import { api, type SnapshotMeta, toCommandError } from "../api";
import { registerAction } from "../actions";
import { defineSetting, getSetting } from "../settings";
import { workbench } from "../workbench";
import { preview } from "./GuardDialog";
import { t, plural } from "../i18n";

export const snapshotSetting = defineSetting<boolean>({
  key: "safety.snapshots",
  section: "Safety",
  label: "Keep the rows an UPDATE or DELETE changes, to put them back",
  description: "Read before the statement runs from the editor, up to 10,000 rows; Snapshots… lists them with their restore script.",
  default: true,
  control: { type: "boolean" },
});

/** Takes a snapshot of the rows `sql` is about to change; its id, or `null` when none was
 *  taken (the setting is off, the statement is not one a snapshot reads, or it failed). */
export async function snapshotBefore(sessionId: number, database: string | null, sql: string): Promise<string | null> {
  if (!getSetting(snapshotSetting)) return null;
  const outcome = await api.takeSnapshot(sessionId, database, sql).catch(() => null);
  return outcome?.kind === "taken" ? outcome.snapshot.id : null;
}

/** Opens the statements that put a snapshot's rows back in a new editor, to look at and run. */
export async function openRestore(connectionId: string, database: string | null, id: string) {
  try {
    const statements = await api.snapshotRestore(id);
    const head = `-- ${t("Puts back the rows of snapshot {id}: look them over, then run them.", { id })}\n`;
    workbench().openTab("query", connectionId, { sql: head + statements.map((s) => `${s};`).join("\n"), database: database ?? undefined });
  } catch (e) {
    workbench().showError(toCommandError(e).message);
  }
}

const time = (at: number) => new Date(at).toLocaleString();

/** The snapshots kept, newest first, each with its restore script. */
export function SnapshotsDialog({ onClose }: { onClose: () => void }) {
  const [list, setList] = useState<SnapshotMeta[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () =>
    api.listSnapshots().then(setList, (e) => {
      setList([]);
      setError(toCommandError(e).message);
    });
  useEffect(() => void load(), []);

  async function remove(id: string) {
    try {
      await api.deleteSnapshot(id);
      await load();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide snapshots" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Snapshots before changes")}</h2>
        <p className="muted small">
          {t("The rows each UPDATE or DELETE run from the editor was about to change, as the statements that put them back.")}
        </p>
        {error && <div className="notice error small">{error}</div>}
        {list === null ? (
          <p className="muted small">{t("Loading…")}</p>
        ) : list.length === 0 ? (
          <p className="muted small">{t("No snapshot yet.")}</p>
        ) : (
          <ul className="snapshot-list">
            {list.map((s) => (
              <li key={s.id}>
                <div className="snapshot-head">
                  <strong>
                    {s.verb} {s.table}
                  </strong>
                  <span className="muted">
                    {s.connection}
                    {s.database ? ` · ${s.database}` : ""} · {time(s.taken_at)} · {plural(s.rows, "{n} row", "{n} rows")}
                  </span>
                </div>
                <code>{preview(s.statement)}</code>
                <div className="snapshot-actions">
                  <button type="button" onClick={() => void remove(s.id)}>
                    {t("Delete")}
                  </button>
                  <button
                    type="button"
                    className="primary"
                    onClick={() => {
                      onClose();
                      void openRestore(s.connection_id, s.database, s.id);
                    }}
                  >
                    {t("Open restore script")}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose} autoFocus>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}

registerAction({
  id: "safety.snapshots",
  scope: "global",
  title: "Snapshots before changes…",
  run: () => workbench().openDialog((close) => createElement(SnapshotsDialog, { onClose: close })),
});
