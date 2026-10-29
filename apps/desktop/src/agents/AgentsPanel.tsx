import { useEffect, useState } from "react";
import {
  api,
  toCommandError,
  type AgentBlocker,
  type AgentSetup,
  type SavedConnection,
  type StaleRegistration,
} from "../api";
import { isProduction } from "../connections";
import { agentAccessLabel, effectiveAgentAccess } from "./access";
import { AgentActivity } from "./AgentActivity";
import { AgentBridge } from "./AgentBridge";
import { AgentClients } from "./AgentClients";
import { CopyBlock } from "./CopyBlock";
import { t, tNodes } from "../i18n";

type Page = "setup" | "activity" | "security";

/** What the row of a connection agents cannot open says after its access. */
function blockerNote(blocker: AgentBlocker): string {
  return blocker === "asks_password"
    ? t("asks for its password at each connection: agents cannot open it")
    : t("password locked by the master password: agents cannot open it");
}

/** A registration that still starts the old server, and how to replace it. */
function StaleNotice({ stale }: { stale: StaleRegistration }) {
  const scope = stale.project ? "local" : "user";
  return (
    <div className="notice warn small" data-stale-registration={stale.name}>
      <p>
        {tNodes(
          "{agent} still starts {command} as {name}: the MCP server of an earlier version, which this version no longer installs. Remove it and register PlatypusDB again as shown above.",
          { agent: stale.agent, command: <code>{stale.command}</code>, name: <code>{stale.name}</code> },
        )}
      </p>
      {stale.agent === "Claude Code" ? (
        <CopyBlock
          label={stale.project ? t("In the folder {folder}", { folder: stale.project }) : t("Remove")}
          text={`claude mcp remove "${stale.name}" -s ${scope}`}
          field="stale_remove"
        />
      ) : (
        <p>{t("Replace its table in ~/.codex/config.toml with the one above.")}</p>
      )}
    </div>
  );
}

/** The access each connection gives agents. */
function AgentSecurity({
  connections,
  blockers,
}: {
  connections: SavedConnection[];
  blockers: Record<string, AgentBlocker>;
}) {
  return (
    <>
      <table className="agents-connections">
        <thead>
          <tr>
            <th>{t("Connection")}</th>
            <th>{t("Agent access")}</th>
          </tr>
        </thead>
        <tbody>
          {connections.map((c) => {
            const access = effectiveAgentAccess(c);
            const blocker = access === "off" ? undefined : blockers[c.id];
            return (
              <tr key={c.id} data-connection={c.name}>
                <td>{c.name}</td>
                <td className={access === "off" ? "muted" : undefined}>
                  {agentAccessLabel(access)}
                  {isProduction(c) && access !== "off" && <span className="muted small"> · {t("production")}</span>}
                  {blocker && (
                    <span className="small agent-blocked" data-blocker={blocker}>
                      {" · "}
                      {blockerNote(blocker)}
                    </span>
                  )}
                </td>
              </tr>
            );
          })}
          {connections.length === 0 && (
            <tr>
              <td colSpan={2} className="muted">
                {t("No saved connections yet.")}
              </td>
            </tr>
          )}
        </tbody>
      </table>
      <p className="muted small">
        {t(
          "Change a connection's access in its settings, under Agent access. Read only is the default; production and read-only connections are never more than read only.",
        )}
      </p>
    </>
  );
}

/** The Agents page of the Settings: how to connect agents (Setup), what they did
 *  (Activity) and the access each connection gives them (Security). */
export function AgentsPanel() {
  const [page, setPage] = useState<Page>("setup");
  const [setup, setSetup] = useState<AgentSetup | null>(null);
  const [connections, setConnections] = useState<SavedConnection[]>([]);
  const [blockers, setBlockers] = useState<Record<string, AgentBlocker>>({});
  const [vault, setVault] = useState(false);
  const [stale, setStale] = useState<StaleRegistration[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const failed = (e: unknown) => setError(toCommandError(e).message);
    api.agentSetup().then(setSetup, failed);
    api.listConnections().then(setConnections, failed);
    api.agentBlockers().then(setBlockers, failed);
    api.vaultStatus().then((v) => setVault(v.enabled), failed);
    api.staleAgentRegistrations().then(setStale, failed);
  }, []);

  const tab = (id: Page, label: string) => (
    <button type="button" role="tab" aria-selected={page === id} data-agents-page={id} onClick={() => setPage(id)}>
      {label}
    </button>
  );

  // Every page stays in the document, the inactive ones hidden.
  return (
    <div className="agents-panel">
      <p className="muted small">
        {tNodes(
          "Agents such as Claude Code, Codex or Cursor can use your saved connections through PlatypusDB's MCP server: the app itself, started with {mcp}. The agent starts it by itself, without a window: PlatypusDB does not need to be open, unless agents must ask before their changes (Settings › Safety). Agents read the schema and run queries with the access each connection gives them; passwords never reach them. Their queries are in the history, with the agent's name.",
          { mcp: <code>--mcp</code> },
        )}
      </p>
      {error && <div className="notice error small">{error}</div>}
      {vault && (
        <div className="notice small" data-notice="vault">
          {t(
            "The master password is on: agents cannot read the passwords saved in PlatypusDB. Give the connections agents use a password command or .pgpass.",
          )}
        </div>
      )}
      <div className="dialog-tabs" role="tablist">
        {tab("setup", t("Setup"))}
        {tab("activity", t("Activity"))}
        {tab("security", t("Security"))}
      </div>
      <div className="dialog-page" data-agents-panel="setup" hidden={page !== "setup"}>
        <AgentClients />
        {setup && (
          <>
            <CopyBlock label="Claude Code" text={setup.claude_command} field="claude_command" />
            <CopyBlock label="Codex (~/.codex/config.toml)" text={setup.codex_config} field="codex_config" />
          </>
        )}
        {stale.map((s) => (
          <StaleNotice key={`${s.agent}:${s.project ?? ""}:${s.name}`} stale={s} />
        ))}
        <AgentBridge />
      </div>
      <div className="dialog-page" data-agents-panel="activity" hidden={page !== "activity"}>
        {page === "activity" && <AgentActivity />}
      </div>
      <div className="dialog-page" data-agents-panel="security" hidden={page !== "security"}>
        <AgentSecurity connections={connections} blockers={blockers} />
      </div>
    </div>
  );
}
