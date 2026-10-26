import type { GuardRequest } from "./guards";
import { t, plural } from "../i18n";

/** Statements listed at most; the rest are counted. */
const SHOWN = 20;


export function preview(sql: string) {
  const line = sql.replace(/\s+/g, " ").trim();
  return line.length > 160 ? `${line.slice(0, 160)}…` : line;
}

/** Asks before risky statements run, or says why they will not. */
export function GuardDialog({ request, onAnswer }: { request: GuardRequest; onAnswer: (run: boolean) => void }) {
  const { connection, unfiltered, production, blocked } = request;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onAnswer(false)}>
      <div className="modal wide guard" onKeyDown={(e) => e.key === "Escape" && onAnswer(false)}>
        <h2>{production.length ? t("Changes on {name} (production)", { name: connection.name }) : t("Change every row?")}</h2>
        {unfiltered.length > 0 && (
          <div className="notice warn small">
            {unfiltered.map((u, i) => (
              <div key={i}>
                <strong>{preview(u.sql).split(" ")[0]?.toUpperCase()}</strong> {t("without WHERE changes every row of")}{" "}
                <code>{u.table}</code>
                {u.rows === null ? ` ${t("(row count unknown)")}` : `: ${plural(u.rows, "about {n} row", "about {n} rows")}`}.
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
