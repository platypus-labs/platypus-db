/**
 * The SQL agents propose with the MCP tool `propose_sql`: the server keeps it in the profile,
 * and the app opens each proposal in a new editor tab on its connection, for the user to
 * review and run by hand.
 */
import { api, type AgentProposal } from "../api";
import { workbench } from "../workbench";
import { t } from "../i18n";

/** How often the app looks for new proposals. */
const POLL_MS = 2000;

/** The SQL as the tab opens it: a comment saying who proposed it, then the SQL. */
export function proposalSql(p: AgentProposal): string {
  const note = t("Proposed by {agent}: review it, then run it yourself.", { agent: p.agent });
  return `-- ${note}\n${p.sql}\n`;
}

function open(proposals: AgentProposal[]) {
  const wb = workbench();
  const connections = wb.connections();
  for (const p of proposals) {
    if (!connections.some((c) => c.id === p.connection_id)) {
      wb.showError(t("{agent} proposed SQL on a connection that is no longer saved.", { agent: p.agent }));
      continue;
    }
    wb.openTab("query", p.connection_id, {
      sql: proposalSql(p),
      title: p.title ?? t("Proposed by {agent}", { agent: p.agent }),
    });
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
      if (proposals.length) open(proposals);
    } catch {
      // No workbench yet, or the log cannot be read: the next round tries again.
    } finally {
      busy = false;
    }
  };
  setInterval(() => void poll(), POLL_MS);
  document.addEventListener("visibilitychange", () => void poll());
}
