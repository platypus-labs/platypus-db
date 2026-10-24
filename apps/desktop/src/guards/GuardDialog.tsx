import type { GuardRequest } from "./guards";

/** Statements listed at most; the rest are counted. */
const SHOWN = 20;

const fmt = new Intl.NumberFormat();

function preview(sql: string) {
  const line = sql.replace(/\s+/g, " ").trim();
  return line.length > 160 ? `${line.slice(0, 160)}…` : line;
}

/** Asks before risky statements run, or says why they will not. */
export function GuardDialog({ request, onAnswer }: { request: GuardRequest; onAnswer: (run: boolean) => void }) {
  const { connection, unfiltered, production, blocked } = request;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onAnswer(false)}>
      <div className="modal wide guard" onKeyDown={(e) => e.key === "Escape" && onAnswer(false)}>
        <h2>{production.length ? `Changes on ${connection.name} (production)` : "Change every row?"}</h2>
        {unfiltered.length > 0 && (
          <div className="notice warn">
            {unfiltered.map((u, i) => (
              <div key={i}>
                <strong>{preview(u.sql).split(" ")[0]?.toUpperCase()}</strong> without WHERE changes every row of{" "}
                <code>{u.table}</code>
                {u.rows === null ? " (row count unknown)" : `: about ${fmt.format(u.rows)} ${u.rows === 1 ? "row" : "rows"}`}.
              </div>
            ))}
          </div>
        )}
        {blocked && <p className="error-text small">The Safety settings do not allow running it. Add a WHERE, or change the setting.</p>}
        {production.length > 0 && (
          <>
            <p className="muted small">
              {production.length} {production.length === 1 ? "statement changes" : "statements change"} data or schema:
            </p>
            <ol className="guard-statements">
              {production.slice(0, SHOWN).map((sql, i) => (
                <li key={i}>
                  <code>{preview(sql)}</code>
                </li>
              ))}
            </ol>
            {production.length > SHOWN && <p className="muted small">…and {production.length - SHOWN} more.</p>}
          </>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={() => onAnswer(false)} autoFocus>
            {blocked ? "Close" : "Cancel"}
          </button>
          {!blocked && (
            <button type="button" className="primary danger" onClick={() => onAnswer(true)}>
              Run
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
