/**
 * What agents propose with the MCP tools `propose_sql` and `propose_edits`: the server keeps
 * it in the profile, and the app opens each proposal in a new tab on its connection, for the
 * user to review. SQL opens in the editor, to run by hand; rows to change open in the table's
 * data viewer as pending edits, to save or discard. Nothing runs or is saved unasked.
 */
import { api, type AgentProposal, isCassandra, isMongo, isRedis, isSearch, type SavedConnection, toCommandError } from "../api";
import { workbench } from "../workbench";
import { plural, t } from "../i18n";
import { ask } from "../ui/ask";
import { floods, lately } from "./flood";

/** How often the app looks for new proposals. */
const POLL_MS = 2000;

/** When the proposals opened lately were opened. */
let recent: number[] = [];

/** The SQL as the tab opens it: a comment saying who proposed it, then the SQL. */
export function proposalSql(p: AgentProposal): string {
  const note = t("Proposed by {agent}: review it, then run it yourself.", { agent: p.agent });
  return `-- ${note}\n${p.sql}\n`;
}

const proposalTitle = (p: AgentProposal) => p.title ?? t("Proposed by {agent}", { agent: p.agent });

/** Rows to change open in the table's data viewer; the viewer's statement is written by the
 *  app's backend from the table's catalog after it checked the proposal, never from its text. */
async function openEdits(p: AgentProposal, edits: string, connection: SavedConnection) {
  const wb = workbench();
  const refused = (message: string) => wb.showError(t("{agent} proposed changes that could not be opened: {message}", { agent: p.agent, message }));
  const driver = connection.driver;
  if (isMongo(driver) || isRedis(driver) || isSearch(driver) || isCassandra(driver)) {
    return refused(t("the rows of {name} are not edited by key", { name: connection.name }));
  }
  try {
    const session = await wb.sideSession(connection);
    if (session === null) return;
    const view = await api.proposedViewer(session, edits);
    wb.openTab("query", connection.id, {
      sql: view.sql,
      autorun: true,
      title: proposalTitle(p),
      table: view.table,
      tableRef: { schema: view.edits.schema, name: view.edits.table, kind: "table" },
      proposed: view.edits,
      proposedBy: p.agent,
    });
  } catch (e) {
    refused(toCommandError(e).message);
  }
}

/** The question when proposals come in a flood. */
function floodQuestion(proposals: AgentProposal[]): string {
  const agents = [...new Set(proposals.map((p) => p.agent))].join(", ");
  const vars = { agent: agents, count: proposals.length };
  if (proposals.every((p) => p.edits !== null)) {
    return plural(
      proposals.length,
      "{agent} proposed {n} change to table data. Open it in its data viewer? Otherwise it is discarded.",
      "{agent} proposed {n} changes to table data. Open each in its data viewer? Otherwise they are discarded.",
      { agent: agents },
    );
  }
  if (proposals.every((p) => p.edits === null)) {
    return t("{agent} proposed {count} SQL scripts. Open each in a new editor tab? Otherwise they are discarded.", vars);
  }
  return t("{agent} proposed {count} SQL scripts or changes to data. Open each in a new tab? Otherwise they are discarded.", vars);
}

async function open(proposals: AgentProposal[]) {
  const wb = workbench();
  // A connection saved since this window last listed them: list them again before giving up.
  if (proposals.some((p) => !wb.connections().some((c) => c.id === p.connection_id))) {
    wb.setConnections(await api.listConnections());
  }
  const now = Date.now();
  if (floods(recent, proposals.length, now)) {
    if (!(await ask(floodQuestion(proposals), { confirm: t("Open them") }))) return;
  }
  recent = [...lately(recent, now), ...proposals.map(() => now)];
  const connections = wb.connections();
  for (const p of proposals) {
    const connection = connections.find((c) => c.id === p.connection_id);
    if (!connection) {
      wb.showError(
        p.edits !== null
          ? t("{agent} proposed changes to data on a connection that is no longer saved.", { agent: p.agent })
          : t("{agent} proposed SQL on a connection that is no longer saved.", { agent: p.agent }),
      );
      continue;
    }
    if (p.edits !== null) {
      await openEdits(p, p.edits, connection);
      continue;
    }
    wb.openTab("query", p.connection_id, { sql: proposalSql(p), title: proposalTitle(p) });
  }
}

/** Looks for proposals while the app runs, from when its shell is ready. */
export function startAgentProposals() {
  let busy = false;
  const poll = async () => {
    if (busy || document.hidden) return;
    busy = true;
    try {
      // Before the shell mounts and lists the connections, the proposals wait.
      if (workbench().connections().length === 0) return;
      const proposals = await api.takeAgentProposals();
      if (proposals.length) await open(proposals);
    } catch {
      // No workbench yet, or the log cannot be read: the next round tries again.
    } finally {
      busy = false;
    }
  };
  setInterval(() => void poll(), POLL_MS);
  document.addEventListener("visibilitychange", () => void poll());
}
