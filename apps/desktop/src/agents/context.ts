/**
 * What an agent may see of the user's work, with their consent (the bridge's
 * `agent-context-request` events): the active tab's editor text, selection and result, as
 * the dialog lists them and as they go to the agent. Pure apart from the registry of the
 * tabs that can be read: the unit tests load it as it is.
 */

/** What of the work an agent asks for (`platypus_mcp::ContextPart`). */
export type ContextPart = "editor" | "selection" | "result";

/** A request to see the user's work (`platypus_mcp::ContextRequest`). */
export interface ContextRequest {
  id: string;
  /** The agent, as it names itself. */
  agent: string;
  /** The run of the agent's server: "share for this session" holds for it. */
  session: string;
  parts: ContextPart[];
}

/** The rows of the shown result (`platypus_mcp::ContextResult`). */
export interface ContextResult {
  columns: string[];
  /** Text as the database gave it; `null` is SQL NULL. */
  rows: (string | null)[][];
  /** The result has more rows than these. */
  truncated: boolean;
  /** How many rows the whole result has; left out while that is not known (still fetching). */
  total_rows?: number;
}

/** What is shared (`platypus_mcp::UserContext`): a part not asked for, or that the tab lacks,
 *  is `null`. */
export interface UserContext {
  connection: string | null;
  connection_id: string | null;
  tab: string | null;
  editor: string | null;
  selection: string | null;
  result: ContextResult | null;
}

/** The user's answer (`platypus_mcp::ContextVerdict`). */
export type ContextVerdict =
  | { answer: "once"; context: UserContext }
  | { answer: "session"; context: UserContext }
  | { answer: "deny"; reason: string | null };

/** What the active tab holds, as its editor and grid show it now. */
export interface TabSnapshot {
  connection: string;
  connectionId: string;
  /** The tab's title, where it has one. */
  title: string | null;
  editor: string;
  /** The selected part of `editor`; empty when nothing is selected. */
  selection: string;
  result: ContextResult | null;
  /** The tab's connection is closed to agents: none of its content may be shared. */
  closedToAgents?: boolean;
}

/** A tab whose work can be shared: the SQL editors register themselves. */
export interface ContextHost {
  tabKey: number;
  /** Reads the tab now. */
  snapshot(): Promise<TabSnapshot>;
}

/** Rows of the result that go to the agent at most (`CONTEXT_ROWS` in the MCP server). */
export const RESULT_ROWS = 100;
/** Bytes of the editor text and of the selection the agent gets at most
 *  (`CONTEXT_TEXT_BYTES`); the page sends up to `SENT_CHARS` and the server cuts and says so. */
export const AGENT_TEXT_BYTES = 64 * 1024;
const SENT_CHARS = 1024 * 1024;

const hosts = new Set<ContextHost>();
let activeKey: number | null = null;

/** Makes a tab readable until the returned function is called. */
export function provideContext(host: ContextHost): () => void {
  hosts.add(host);
  return () => {
    hosts.delete(host);
  };
}

/** Tells which tab is the active one (`null`: none). */
export function followActiveTab(key: number | null) {
  activeKey = key;
}

/** What the active tab holds; `null` when it is not an SQL editor, or there is none. */
export async function readActive(): Promise<TabSnapshot | null> {
  const host = [...hosts].find((h) => h.tabKey === activeKey);
  return host ? host.snapshot() : null;
}

/** The part of `text` selected by `from..to`, in either order and within the text. */
export function selectionText(text: string, from: number, to: number): string {
  const [start, end] = from <= to ? [from, to] : [to, from];
  return text.slice(Math.max(0, start), Math.max(0, end));
}

/** The first `RESULT_ROWS` rows of a result read column by column, as rows. `total` is how
 *  many rows the result has loaded, `more` whether the server has more (then the size of the
 *  whole result is not known, and is left out). */
export function resultRows(names: readonly string[], columns: readonly (readonly (string | null)[])[], total: number, more: boolean): ContextResult {
  const count = Math.min(total, RESULT_ROWS, columns[0]?.length ?? 0);
  const rows = Array.from({ length: count }, (_, r) => columns.map((c) => c[r] ?? null));
  return { columns: [...names], rows, truncated: more || total > count, ...(more ? {} : { total_rows: total }) };
}

/** What goes to the agent: the parts asked for, of `snapshot` (all empty without one). */
export function buildContext(parts: readonly ContextPart[], snapshot: TabSnapshot | null): UserContext {
  // A connection closed to agents shares nothing: the server refuses it too, but the content
  // must not even leave the page.
  const asked = (part: ContextPart) => parts.includes(part) && !snapshot?.closedToAgents;
  const text = (value: string) => value.slice(0, SENT_CHARS);
  return {
    connection: snapshot?.connection ?? null,
    connection_id: snapshot?.connectionId ?? null,
    tab: snapshot?.title ?? null,
    editor: snapshot && asked("editor") ? text(snapshot.editor) : null,
    selection: snapshot && asked("selection") && snapshot.selection !== "" ? text(snapshot.selection) : null,
    result: snapshot && asked("result") ? snapshot.result : null,
  };
}

/** One part in the dialog's list: whether the tab has it and how much. */
export interface ContextItem {
  part: ContextPart;
  present: boolean;
  /** Characters of the editor text or the selection. */
  chars: number;
  /** The rows of the whole result when known, else those the page has read. */
  rows: number;
  /** `rows` is the size of the whole result, not a lower bound (still fetching, or capped). */
  exact: boolean;
  columns: number;
  /** The agent gets less than this: the text beyond `AGENT_TEXT_BYTES`, the rows beyond the first. */
  cut: boolean;
}

/** What the dialog tells the user would be shared. */
export interface ContextSummary {
  /** The active tab is an SQL editor. */
  tab: boolean;
  title: string | null;
  connection: string | null;
  /** The tab is on a connection closed to agents: nothing of it can be shared. */
  closed: boolean;
  items: ContextItem[];
}

/** The parts asked for, as `buildContext` would send them. */
export function summarize(parts: readonly ContextPart[], snapshot: TabSnapshot | null): ContextSummary {
  const context = buildContext(parts, snapshot);
  const bytes = (text: string) => new TextEncoder().encode(text).length;
  const items = ([...new Set(parts)] as ContextPart[]).map((part): ContextItem => {
    const item = { part, present: false, chars: 0, rows: 0, exact: true, columns: 0, cut: false };
    if (part === "result") {
      const r = context.result;
      if (!r) return item;
      const exact = r.total_rows !== undefined || !r.truncated;
      return { ...item, present: true, rows: r.total_rows ?? r.rows.length, exact, columns: r.columns.length, cut: r.truncated };
    }
    const text = context[part];
    return text === null ? item : { ...item, present: true, chars: text.length, cut: bytes(text) > AGENT_TEXT_BYTES };
  });
  return { tab: snapshot !== null, title: context.tab, connection: context.connection, closed: snapshot?.closedToAgents === true, items };
}

/** Why a tab on a connection closed to agents is refused; the agent reads it. */
export const CLOSED_REASON = "the active tab is on a connection closed to agents";

/** The answer that shares `snapshot` (what the user was shown, or the tab as it is now): a
 *  refusal for a tab on a connection closed to agents, whatever was allowed before. */
export function verdictFor(parts: readonly ContextPart[], snapshot: TabSnapshot | null, how: "once" | "session"): ContextVerdict {
  if (snapshot?.closedToAgents) return { answer: "deny", reason: CLOSED_REASON };
  return { answer: how, context: buildContext(parts, snapshot) };
}

/** Reads the active tab and builds what goes to the agent. */
export async function shareContext(parts: readonly ContextPart[]): Promise<UserContext> {
  return buildContext(parts, await readActive());
}

/** The agent sessions the user let see their work until they stop (or the app restarts). */
export type Grants = ReadonlySet<string>;

export const noGrants: Grants = new Set();

export function grant(grants: Grants, session: string): Grants {
  return new Set(grants).add(session);
}

export function granted(grants: Grants, session: string): boolean {
  return grants.has(session);
}
