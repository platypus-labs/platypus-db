import { useEffect, useState } from "react";
import { toCommandError } from "../api";
import { plural, t } from "../i18n";
import { answerContext, shareWith, useWaitingContexts } from "./bridge";
import { type ContextItem, type ContextSummary, readActive, summarize } from "./context";
import { denial, timeLeft } from "./writes";

/** What the agent would get of one part, in words. */
function line(item: ContextItem): string {
  const characters = plural(item.chars, "{n} character", "{n} characters");
  switch (item.part) {
    case "editor":
      return item.present && item.chars > 0 ? t("The editor's text: {size}", { size: characters }) : t("The editor's text: nothing");
    case "selection":
      return item.present && item.chars > 0 ? t("The selection: {size}", { size: characters }) : t("The selection: nothing selected");
    case "result":
      return item.present
        ? t("The result: {rows}, {columns}", {
            rows: plural(item.rows, "{n} row", "{n} rows"),
            columns: plural(item.columns, "{n} column", "{n} columns"),
          })
        : t("The result: none in this tab");
  }
}

/** The note under a part the agent gets only the start of. */
function cutNote(item: ContextItem): string {
  return item.part === "result"
    ? t("The result has more rows: the agent gets the first {n}.", { n: item.rows })
    : t("Only the first 64 KiB goes to the agent.");
}

/** Asks the user about the oldest request of an agent to see what they work on: which tab
 *  and connection, and which of the editor text, selection and result would be shared, as of
 *  now. Refusing (Escape too) can say why: the agent reads it. Nothing answered in five
 *  minutes is refused by the bridge. */
export function AgentContextDialog() {
  const waiting = useWaitingContexts();
  const first = waiting[0];
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);
  // An answer on its way: a second click would answer again.
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [summary, setSummary] = useState<ContextSummary | null>(null);
  const open = first !== undefined;
  useEffect(() => {
    if (!open) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [open]);
  // Each request starts without the reason typed for the one before, and reads the tab anew.
  const id = first?.request.id;
  const parts = first?.request.parts;
  useEffect(() => {
    setReason("");
    setError(null);
    setBusy(false);
    setSummary(null);
    if (!parts) return;
    let live = true;
    readActive().then(
      (snapshot) => live && setSummary(summarize(parts, snapshot)),
      () => live && setSummary(summarize(parts, null)),
    );
    return () => {
      live = false;
    };
  }, [id, parts]);
  if (!first) return null;
  const { request, since } = first;
  const run = (action: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    action().catch((e: unknown) => {
      setError(toCommandError(e).message);
      setBusy(false);
    });
  };
  const deny = () => run(() => answerContext(request.id, denial(reason)));
  const share = (how: "once" | "session") => run(() => shareWith(request, how));
  return (
    <div className="modal-backdrop">
      <div
        className="modal wide guard agent-context"
        role="alertdialog"
        aria-labelledby="agent-context-title"
        onKeyDown={(e) => e.key === "Escape" && deny()}
      >
        <h2 id="agent-context-title">{t("{agent} wants to see what you are working on", { agent: request.agent })}</h2>
        <div data-field="tab">
          {summary === null
            ? t("Reading the active tab…")
            : !summary.tab
              ? t("The active tab is not an SQL editor, so there is nothing of it to share.")
              : summary.title
                ? t("Active tab: {tab}, on {name}", { tab: summary.title, name: summary.connection ?? "" })
                : t("Active tab, on {name}", { name: summary.connection ?? "" })}
        </div>
        {summary?.tab && (
          <ul className="agent-context-items" data-field="items">
            {summary.items.map((item) => (
              <li key={item.part} data-part={item.part} data-present={item.present && (item.chars > 0 || item.part === "result")}>
                {line(item)}
                {item.present && item.cut && <div className="muted small">{cutNote(item)}</div>}
              </li>
            ))}
          </ul>
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
            onClick={() => share("session")}
            disabled={busy || summary === null}
            title={t("This request, and the next ones of this agent until it stops: the tab is read again each time")}
            data-answer="session"
          >
            {t("Share for this session")}
          </button>
          <button type="button" className="primary" onClick={() => share("once")} disabled={busy || summary === null} data-answer="once">
            {t("Share once")}
          </button>
        </div>
      </div>
    </div>
  );
}
