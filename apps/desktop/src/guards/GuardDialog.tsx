import type { GuardRequest, UnfilteredChange } from "./guards";
import { t, plural } from "../i18n";

/** Statements listed at most; the rest are counted. */
const SHOWN = 20;


export function preview(sql: string) {
  const line = sql.replace(/\s+/g, " ").trim();
  return line.length > 160 ? `${line.slice(0, 160)}…` : line;
}

/** What a change does to its table, after its verb: TRUNCATE has no WHERE, MongoDB has
 *  filters and documents, SQL Server's TOP picks some rows. */
function changeText(u: UnfilteredChange) {
  if (u.verb === "TRUNCATE") return t("empties");
  // Redis's FLUSHDB and FLUSHALL name no table: they empty whole databases.
  if (u.verb === "FLUSHDB") return t("deletes every key of the session's database");
  if (u.verb === "FLUSHALL") return t("deletes every key of every database");
  // MariaDB's DELETE HISTORY leaves the current rows as they are.
  if (u.verb === "DELETE HISTORY") return t("erases every past version of the rows of");
  if (u.documents) return t("with an empty filter changes every document of");
  if (u.limited) return t("without WHERE changes rows the database picks in");
  return t("without WHERE changes every row of");
}

/** Asks before risky statements run, or says why they will not. `named` says on which
 *  connection, for a run on several. */
export function GuardDialog({ request, onAnswer, named = false }: { request: GuardRequest; onAnswer: (run: boolean) => void; named?: boolean }) {
  const { connection, unfiltered, production, blocked } = request;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onAnswer(false)}>
      <div className="modal wide guard" onKeyDown={(e) => e.key === "Escape" && onAnswer(false)}>
        <h2>
          {production.length
            ? t("Changes on {name} (production)", { name: connection.name })
            : named
              ? t("Change every row on {name}?", { name: connection.name })
              : t("Change every row?")}
        </h2>
        {unfiltered.length > 0 && (
          <div className="notice warn small">
            {unfiltered.map((u, i) => (
              <div key={i}>
                <strong>{u.limited ? `${u.verb} TOP` : u.verb}</strong> {changeText(u)}
                {u.table && (
                  <>
                    {" "}
                    <code>{u.table}</code>
                    {u.rows === null
                      ? ` ${t("(row count unknown)")}`
                      : `: ${u.documents ? plural(u.rows, "about {n} document", "about {n} documents") : plural(u.rows, "about {n} row", "about {n} rows")}`}
                  </>
                )}
                .
              </div>
            ))}
          </div>
        )}
        {blocked && <div className="notice error small">{t("The Safety settings do not allow running it. Add a WHERE, or change the setting.")}</div>}
        {production.length > 0 && (
          <>
            <p className="muted small">{plural(production.length, "{n} statement changes data or schema:", "{n} statements change data or schema:")}</p>
            <ol className="guard-statements">
              {production.slice(0, SHOWN).map((sql, i) => (
                <li key={i}>
                  <code>{preview(sql)}</code>
                </li>
              ))}
            </ol>
            {production.length > SHOWN && <p className="muted small">{t("…and {n} more.", { n: production.length - SHOWN })}</p>}
          </>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={() => onAnswer(false)} autoFocus>
            {t(blocked ? "Close" : "Cancel")}
          </button>
          {!blocked && (
            <button type="button" className="primary danger" onClick={() => onAnswer(true)}>
              {t("Run")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
