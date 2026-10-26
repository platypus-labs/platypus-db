import { useEffect, useState } from "react";
import { api, type LintFinding, type LintKind, type SavedConnection, toCommandError } from "../api";
import { workbench } from "../workbench";
import { t, plural, msg } from "../i18n";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  onClose: () => void;
}

const KIND_LABELS: Record<LintKind, string> = {
  no_primary_key: msg("No primary key"),
  unindexed_foreign_key: msg("Foreign key without index"),
  duplicate_index: msg("Duplicate index"),
  redundant_index: msg("Redundant index"),
  foreign_key_type_mismatch: msg("Foreign key type mismatch"),
  unreadable: msg("Could not be read"),
};

/** The schema linter's findings: tables without a primary key, foreign keys without an index,
 *  duplicate or redundant indexes, foreign keys whose type differs from the referenced column.
 *  The fixes open in an SQL editor, to review and run there. */
export function LintDialog({ connection, database, schema, onClose }: Props) {
  const [findings, setFindings] = useState<LintFinding[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    workbench()
      .sideSession(connection, database)
      .then(async (sid) => {
        if (sid === null) return onClose();
        setFindings(await api.lintSchema(sid, schema));
      })
      .catch((e) => setError(toCommandError(e).message));
  }, []);

  const fixes = (findings ?? []).flatMap((f) => (f.fix ? [`${f.fix};`] : []));
  const openSql = (sql: string) => {
    workbench().openTab("query", connection.id, { sql, database });
    onClose();
  };
  const openTable = (f: LintFinding) => {
    const section = f.kind === "no_primary_key" || f.kind === "unreadable" ? "columns" : f.kind.endsWith("_index") ? "indexes" : "foreign_keys";
    workbench().openTab("table-editor", connection.id, { database, schema, table: f.table, title: f.table, section });
    onClose();
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal schema-lint" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Check schema {schema}", { schema })}</h2>
        {error ? (
          <div className="notice error small">{error}</div>
        ) : findings === null ? (
          <p className="muted small">{t("Reading the tables…")}</p>
        ) : findings.length === 0 ? (
          <p className="muted small">{t("No problems found.")}</p>
        ) : (
          <>
            <p className="muted small">{plural(findings.length, "{n} problem found.", "{n} problems found.")}</p>
            <ul className="lint-findings">
              {findings.map((f, i) => (
                <li key={i}>
                  <div className="lint-head">
                    <span className={`lint-kind ${f.kind}`}>{t(KIND_LABELS[f.kind])}</span>
                    <button type="button" className="link" onClick={() => openTable(f)}>
                      {f.object ? `${f.table} · ${f.object}` : f.table}
                    </button>
                  </div>
                  <div className="small">{f.message}</div>
                  {f.fix && (
                    <div className="lint-fix">
                      <code>{f.fix}</code>
                      <button type="button" className="link" onClick={() => openSql(`${f.fix};\n`)}>
                        {t("Open in editor")}
                      </button>
                    </div>
                  )}
                </li>
              ))}
            </ul>
          </>
        )}
        <div className="actions">
          <span className="spacer" />
          {fixes.length > 0 && (
            <button type="button" onClick={() => openSql(`${fixes.join("\n")}\n`)}>
              {plural(fixes.length, "Open {n} fix in editor", "Open {n} fixes in editor")}
            </button>
          )}
          <button type="button" className="primary" onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
