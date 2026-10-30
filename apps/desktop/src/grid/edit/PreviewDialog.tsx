import { t, plural } from "../../i18n";

interface Props {
  statements: string[];
  busy: boolean;
  /** Saving changes a production database: said in the dialog. */
  production?: boolean;
  /** Commands rather than SQL statements: Redis's, sent as one MULTI/EXEC, Elasticsearch's
   *  requests, sent one after the other, or Cassandra's statements, which run one after the
   *  other too (there is no transaction to undo them with). */
  commands?: "multi" | "requests" | "statements";
  onSave: () => void;
  onClose: () => void;
}

/** The statements that saving would run, before they run. */
export function PreviewDialog({ statements, busy, production = false, commands, onSave, onClose }: Props) {
  // A request may have a body: a blank line ends it.
  const script =
    commands === "requests" ? statements.join("\n\n") : statements.map((s) => (commands ? s : `${s};`)).join("\n");
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Changes to save")}</h2>
        {production && <div className="notice warn small">{t("This is a production connection: check the SQL before saving.")}</div>}
        <p className="muted small">
          {commands === "requests"
            ? plural(
                statements.length,
                "{n} request runs on the server: a request the server refuses stops the rest, and the ones before it stay.",
                "{n} requests run one after the other: a request the server refuses stops the rest, and the ones before it stay.",
              )
            : commands === "statements"
            ? plural(
                statements.length,
                "{n} statement runs on the server: a statement the server refuses stops the rest, and the ones before it stay.",
                "{n} statements run one after the other: a statement the server refuses stops the rest, and the ones before it stay.",
              )
            : commands
            ? plural(
                statements.length,
                "{n} command runs in one MULTI/EXEC: a command the server refuses does not undo the others.",
                "{n} commands run in one MULTI/EXEC: a command the server refuses does not undo the others.",
              )
            : plural(
                statements.length,
                "{n} statement runs in one transaction: all of them, or none if one fails.",
                "{n} statements run in one transaction: all of them, or none if one fails.",
              )}
        </p>
        <pre className="sql-preview">{script}</pre>
        <div className="actions">
          <button type="button" onClick={() => void navigator.clipboard.writeText(script)}>
            {t("Copy")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={onClose} autoFocus>
            {t("Close")}
          </button>
          <button type="button" className="primary" onClick={onSave} disabled={busy}>
            {t("Save")}
          </button>
        </div>
      </div>
    </div>
  );
}
