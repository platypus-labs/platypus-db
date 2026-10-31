import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { toCommandError } from "../api";
import { plural, t } from "../i18n";
import { answerWrite, useWaitingWrites } from "./bridge";
import { denial, estimatedRows, statementParts, timeLeft, type StatementPart, type Verdict } from "./writes";

/** A piece of the statement: its text, or a marker in place of padding, which no one could
 *  take for SQL. */
function part(piece: StatementPart, i: number) {
  if ("text" in piece) return <span key={i}>{piece.text}</span>;
  return (
    <span key={i} className="statement-gap" data-gap={piece.gap}>
      {piece.gap === "lines"
        ? plural(piece.count, "{n} blank line", "{n} blank lines")
        : plural(piece.count, "{n} space", "{n} spaces")}
    </span>
  );
}

/** Asks the user about the oldest change an agent wants to run: its statement (padding
 *  collapsed, and allowed only once read to its end when taller than its box), the rows
 *  the database expects it to change and its plan. Refusing (Escape too) can say why: the
 *  agent reads it. Nothing answered in five minutes is refused by the bridge. */
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
  // A statement taller than its box can be allowed once it was read to its end.
  const box = useRef<HTMLPreElement>(null);
  const [seenEnd, setSeenEnd] = useState(true);
  const readTo = () => {
    const el = box.current;
    if (el && el.scrollTop + el.clientHeight >= el.scrollHeight - 2) setSeenEnd(true);
  };
  useLayoutEffect(() => {
    const el = box.current;
    setSeenEnd(!el || el.scrollHeight <= el.clientHeight + 1);
  }, [id]);
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
        <pre className="sql-preview" data-field="statement" ref={box} onScroll={readTo}>
          {statementParts(request.statement).map(part)}
        </pre>
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
