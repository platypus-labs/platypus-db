import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, type DriverKind, toCommandError } from "../api";
import { formatIndent } from "../editor/actions";
import { plural, t } from "../i18n";
import { getSetting } from "../settings";
import { workbench } from "../workbench";
import { answerWrite, useWaitingWrites } from "./bridge";
import { colored } from "./statementColors";
import { denial, estimatedRows, sameStatement, statementParts, timeLeft, type StatementPart, type Verdict } from "./writes";

/** A piece of the statement: its text in the editor's colors, or a marker in place of padding
 *  or of an invisible character, which no one could take for SQL. */
function part(piece: StatementPart, i: number, driver: DriverKind | null) {
  if ("text" in piece) return <span key={i}>{colored(piece.text, driver)}</span>;
  if ("invisible" in piece) {
    return (
      <span key={i} className="statement-gap" data-gap="invisible">
        {t("invisible {code}", { code: piece.invisible })}
      </span>
    );
  }
  return (
    <span key={i} className="statement-gap" data-gap={piece.gap}>
      {piece.gap === "lines"
        ? plural(piece.count, "{n} blank line", "{n} blank lines")
        : plural(piece.count, "{n} space", "{n} spaces")}
    </span>
  );
}

/** Asks the user about the oldest change an agent wants to run: its statement (laid out and
 *  colored as in the editor, with what was sent a click away; padding collapsed, and allowed
 *  only once read to its end when taller than its box), the rows the database expects it to
 *  change and its plan. Refusing (Escape too) can say why: the agent reads it. Nothing
 *  answered in five minutes is refused by the bridge. */
export function AgentWriteDialog() {
  const waiting = useWaitingWrites();
  const first = waiting[0];
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  // An answer on its way: a second click would answer again.
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!first) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [first]);
  // Each request starts without the reason typed for the one before.
  const id = first?.request.id;
  useEffect(() => {
    setReason("");
    setError(null);
    setBusy(false);
  }, [id]);
  // The statement laid out by the editor's formatter. Keywords stay as they came, so that only
  // spaces and line breaks can differ from what runs, which `sameStatement` holds it to.
  const connection = first?.request.connection;
  const statement = first?.request.statement;
  // Asked only with a request: the dialog also renders before the workbench is there.
  const driver = first ? (workbench().connections().find((c) => c.name === connection)?.driver ?? null) : null;
  const [laidOut, setLaidOut] = useState<string | null>(null);
  const [asSent, setAsSent] = useState(false);
  useEffect(() => {
    setLaidOut(null);
    setAsSent(false);
    if (statement === undefined || !driver) return;
    let stale = false;
    api.formatSql(statement, driver, { keyword_case: "preserve", indent: getSetting(formatIndent) }).then(
      (text) => {
        if (!stale && text !== statement && sameStatement(statement, text)) setLaidOut(text);
      },
      () => {},
    );
    return () => {
      stale = true;
    };
  }, [id]);
  const shown = !asSent && laidOut !== null ? laidOut : (statement ?? "");
  // A statement taller than its box can be allowed once it was read to its end, as it shows.
  const box = useRef<HTMLPreElement>(null);
  const [seenEnd, setSeenEnd] = useState(true);
  const readTo = () => {
    const el = box.current;
    if (el && el.scrollTop + el.clientHeight >= el.scrollHeight - 2) setSeenEnd(true);
  };
  useLayoutEffect(() => {
    const el = box.current;
    if (el) el.scrollTop = 0;
    setSeenEnd(!el || el.scrollHeight <= el.clientHeight + 1);
  }, [id, shown]);
  if (!first) return null;
  const { request, since } = first;
  const rows = estimatedRows(request.estimated_rows);
  const answer = (verdict: Verdict) => {
    if (busy) return;
    setBusy(true);
    answerWrite(request.id, verdict).catch((e: unknown) => {
      setError(toCommandError(e).message);
      setBusy(false);
    });
  };
  const deny = () => answer(denial(reason));
  return (
    <div className="modal-backdrop">
      <div
        className="modal wide guard agent-write"
        role="alertdialog"
        aria-labelledby="agent-write-title"
        onKeyDown={(e) => e.key === "Escape" && deny()}
      >
        <h2 id="agent-write-title">{t("{agent} wants to change data on {name}", { agent: request.agent, name: request.connection })}</h2>
        <pre className="sql-preview statement-code" data-field="statement" ref={box} onScroll={readTo}>
          {statementParts(shown).map((piece, i) => part(piece, i, driver))}
        </pre>
        {laidOut !== null && (
          <div className="agent-write-layout">
            <button type="button" className="link small" onClick={() => setAsSent((was) => !was)} data-field="layout">
              {asSent ? t("Show it formatted") : t("Show it as the agent sent it")}
            </button>
          </div>
        )}
        {!seenEnd && (
          <div className="notice small warn" data-field="more">
            {t("The statement goes on below: scroll it to the end before allowing it.")}
          </div>
        )}
        <div className={`notice small ${rows === null ? "" : "warn"}`} data-field="rows">
          {rows === null
            ? t("The database gives no estimate of the rows it changes.")
            : plural(rows, "The database expects it to change about {n} row.", "The database expects it to change about {n} rows.")}
        </div>
        {request.plan && (
          <details className="agent-write-plan">
            <summary>{t("Plan")}</summary>
            <pre className="sql-preview" data-field="plan">
              {request.plan}
            </pre>
          </details>
        )}
        <label className="field">
          <span>{t("Reason, if you refuse (the agent reads it)")}</span>
          <input type="text" value={reason} onChange={(e) => setReason(e.target.value)} data-field="reason" />
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="muted small" data-field="left">
            {t("Refused in {time} without an answer", { time: timeLeft(since, now) })}
            {waiting.length > 1 && ` · ${plural(waiting.length - 1, "{n} more waiting", "{n} more waiting")}`}
          </span>
          <span className="spacer" />
          <button type="button" onClick={deny} disabled={busy} autoFocus data-answer="deny">
            {t("Refuse")}
          </button>
          <button
            type="button"
            onClick={() => answer({ answer: "session" })}
            disabled={busy || !seenEnd}
            title={t("This change, and the next ones of this agent on {name} until it stops", { name: request.connection })}
            data-answer="session"
          >
            {t("Allow for this session")}
          </button>
          <button type="button" className="primary danger" onClick={() => answer({ answer: "once" })} disabled={busy || !seenEnd} data-answer="once">
            {t("Allow once")}
          </button>
        </div>
      </div>
    </div>
  );
}
