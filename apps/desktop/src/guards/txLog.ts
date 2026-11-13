/** The transaction log of a tab's session: what ran, when, and in which transaction, with
 *  how each transaction ended. Kept in memory for the tab, the newest last. */

type LogKind = "query" | "dml" | "ddl" | "other";

export interface LogEntry {
  at: number;
  sql: string;
  kind: LogKind;
  /** The statement failed. */
  failed: boolean;
  /** The transaction it ran in; `null` when it committed on its own. */
  tx: number | null;
  /** The snapshot of the rows it changed, taken before it ran. */
  snapshot?: string | null;
}

export interface TxRecord {
  id: number;
  started: number;
  /** When it ended, and how (`ended` when the app cannot tell: a failure, a reconnection,
   *  the database's own rollback); `null` while it is open. */
  ended: number | null;
  end: End | null;
}

export interface TxLog {
  entries: LogEntry[];
  txs: TxRecord[];
  /** The open transaction. */
  open: number | null;
  next: number;
}

/** How many statements the log keeps. */
export const LOG_LIMIT = 500;

export const emptyLog = (): TxLog => ({ entries: [], txs: [], open: null, next: 1 });

type End = "commit" | "rollback" | "ended";

/** How a statement that ended its transaction ended it, as far as its text tells: `COMMIT`
 *  and `END`, DDL that commits by itself, a rollback by name; else the app cannot tell (a
 *  failed statement the database rolled back with its transaction, a reconnection). */
function ending(entry: Omit<LogEntry, "tx">): End {
  if (entry.failed) return "ended";
  if (/^\s*(rollback|abort)\b/i.test(entry.sql) && !/\bto\b/i.test(entry.sql)) return "rollback";
  if (/^\s*(commit|end)\b/i.test(entry.sql) || entry.kind === "ddl") return "commit";
  return "ended";
}

function close(log: TxLog, end: End, at: number): TxLog {
  if (log.open === null) return log;
  const txs = log.txs.map((t) => (t.id === log.open ? { ...t, ended: at, end } : t));
  return { ...log, txs, open: null };
}

/** The log after `sql` ran; `openAfter` is whether a transaction is open after it, `null`
 *  when unknown (a query, which changes nothing: it stays as it was). */
export function noteStatement(log: TxLog, entry: Omit<LogEntry, "tx">, openAfter: boolean | null): TxLog {
  let next = log;
  let tx = log.open;
  if (openAfter === true && log.open === null) {
    // This statement opened it: BEGIN, or the first one in manual commit.
    tx = log.next;
    const txs = [...log.txs, { id: tx, started: entry.at, ended: null, end: null }].slice(-LOG_LIMIT);
    next = { ...log, next: log.next + 1, open: tx, txs };
  }
  next = { ...next, entries: [...next.entries, { ...entry, tx }].slice(-LOG_LIMIT) };
  if (openAfter === false && next.open !== null) next = close(next, ending(entry), entry.at);
  return next;
}

/** The log after Commit or Rollback from the toolbar. */
export const noteEnd = (log: TxLog, commit: boolean, at: number): TxLog => close(log, commit ? "commit" : "rollback", at);

/** The entries grouped as they ran: a transaction's statements together, each statement that
 *  committed on its own alone; the newest group first. */
export function groups(log: TxLog): { tx: TxRecord | null; entries: LogEntry[] }[] {
  const out: { tx: TxRecord | null; entries: LogEntry[] }[] = [];
  for (const entry of log.entries) {
    const last = out[out.length - 1];
    if (entry.tx !== null && last?.tx?.id === entry.tx) last.entries.push(entry);
    else out.push({ tx: entry.tx === null ? null : (log.txs.find((t) => t.id === entry.tx) ?? null), entries: [entry] });
  }
  return out.reverse();
}
