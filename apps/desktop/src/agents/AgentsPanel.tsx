import { useEffect, useState } from "react";
import {
  api,
  toCommandError,
  type AgentAccess,
  type AgentBlocker,
  type SavedConnection,
  type StaleRegistration,
} from "../api";
import { isProduction } from "../connections";
import { DriverIcon } from "../components/DriverPicker";
import { workbench } from "../workbench";
import { AGENT_ACCESS, agentAccessLabel, effectiveAgentAccess, isCapped } from "./access";
import { AgentActivity } from "./AgentActivity";
import { AgentBridge } from "./AgentBridge";
import { AgentClients } from "./AgentClients";
import { PanelResizer, useStoredSize } from "../shell/PanelResizer";
import { CopyBlock } from "./CopyBlock";
import { msg, t, tNodes } from "../i18n";

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

/** What each access level lets agents do, as the detail panel explains it. */
const LEVEL_NOTES: Record<AgentAccess, string> = {
  off: msg("Agents do not see this connection."),
  read_only: msg("They can read the data and the schema, and change nothing."),
  read_write: msg("They can also change data, within the connection's safety settings."),
};

/** The three levels of a connection, the one it has marked. */
function AccessLevels({ connection, onChange }: { connection: SavedConnection; onChange: (access: AgentAccess) => void }) {
  const current = effectiveAgentAccess(connection);
  const kept = !!connection.restrictions?.no_export;
  return (
    <div className="agent-levels" role="radiogroup" aria-label={t("Agent access")}>
      {AGENT_ACCESS.map((a) => (
        <label key={a.value} className={`agent-level${a.value === current ? " on" : ""}`} data-level={a.value}>
          <input
            type="radio"
            name="agent-access"
            value={a.value}
            checked={a.value === current}
            disabled={kept || (a.value === "read_write" && isCapped(connection))}
            onChange={() => onChange(a.value)}
          />
          <div>
            <div className="agent-level-title">{t(a.label)}</div>
            <div className="muted small">{t(LEVEL_NOTES[a.value])}</div>
            {a.value === "read_write" && isCapped(connection) && (
              <div className="small agent-blocked" data-field="level-capped">
                {t("Not available: production and read-only connections stay read only.")}
              </div>
            )}
          </div>
        </label>
      ))}
    </div>
  );
}

/** The access each connection gives agents: a card for each, the selected one explained beside. */
function AgentSecurity({
  connections,
  blockers,
  onSaved,
  onError,
}: {
  connections: SavedConnection[];
  blockers: Record<string, AgentBlocker>;
  onSaved: (all: SavedConnection[]) => void;
  onError: (message: string) => void;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const chosen = connections.find((c) => c.id === selectedId) ?? connections[0];
  const chosenAccess = chosen ? effectiveAgentAccess(chosen) : "off";
  const chosenBlocker = chosen && chosenAccess !== "off" ? blockers[chosen.id] : undefined;
  const [detailWidth, setDetailWidth] = useStoredSize("layout.agents.detail", 260);
  // Enter or Space chooses a row, the arrows move to the next and the previous.
  const rowKeys = (e: React.KeyboardEvent<HTMLTableRowElement>, id: string) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      setSelectedId(id);
      return;
    }
    const to = e.key === "ArrowDown" ? e.currentTarget.nextElementSibling : e.key === "ArrowUp" ? e.currentTarget.previousElementSibling : null;
    if (!(to instanceof HTMLElement) || !to.dataset.id) return;
    e.preventDefault();
    setSelectedId(to.dataset.id);
    to.focus();
  };
  return (
    <>
      <div className="agents-split">
        <table className="agents-connections agent-cards">
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
                <tr
                  key={c.id}
                  data-connection={c.name}
                  data-id={c.id}
                  className={c.id === chosen?.id ? "selected" : undefined}
                  tabIndex={0}
                  aria-selected={c.id === chosen?.id}
                  onClick={() => setSelectedId(c.id)}
                  onKeyDown={(e) => rowKeys(e, c.id)}
                >
                  <td>
                    <DriverIcon driver={c.driver} />
                    <span className="agent-card-name">{c.name}</span>
                  </td>
                  <td className={access === "off" ? "muted" : undefined}>
                    <span className={`agent-dot ${access}`} aria-hidden />
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
        {chosen && (
          <PanelResizer
            storeKey="layout.agents.detail"
            size={detailWidth}
            fallback={260}
            min={200}
            max={() => 560}
            direction="column"
            after
            onResize={setDetailWidth}
            label={t("Resize the details")}
          />
        )}
        {chosen && (
          <aside className="agent-detail" aria-label={t("Agent access")} style={{ width: detailWidth }}>
            <header>
              <DriverIcon driver={chosen.driver} />
              <strong>{chosen.name}</strong>
              {isProduction(chosen) && <span className="badge prod">{t("production")}</span>}
            </header>
            <AccessLevels
              connection={chosen}
              onChange={(agent_access) =>
                api.saveConnection({ ...chosen, agent_access }, { action: "keep" }).then(onSaved, (e: unknown) => onError(toCommandError(e).message))
              }
            />
            {chosenBlocker && <p className="small agent-blocked">{blockerNote(chosenBlocker)}</p>}
          </aside>
        )}
      </div>
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
  const [connections, setConnections] = useState<SavedConnection[]>([]);
  const [blockers, setBlockers] = useState<Record<string, AgentBlocker>>({});
  const [vault, setVault] = useState(false);
  const [stale, setStale] = useState<StaleRegistration[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const failed = (e: unknown) => setError(toCommandError(e).message);
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
      <details className="agents-intro muted small">
        <summary>{t("Agents use your saved connections through PlatypusDB's MCP server. How it works")}</summary>
        <p>
        {tNodes(
          "Agents use your saved connections through PlatypusDB's MCP server: the app itself, started with {mcp}. They start it by themselves, without a window: PlatypusDB need not be open, unless agents must ask before their changes (Settings › Safety). Passwords never reach them, and their queries go into the history with the agent's name.",
          { mcp: <code>--mcp</code> },
        )}
        </p>
      </details>
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
        {stale.map((s) => (
          <StaleNotice key={`${s.agent}:${s.project ?? ""}:${s.name}`} stale={s} />
        ))}
        <AgentBridge />
      </div>
      <div className="dialog-page" data-agents-panel="activity" hidden={page !== "activity"}>
        {page === "activity" && <AgentActivity onSetup={() => setPage("setup")} />}
      </div>
      <div className="dialog-page" data-agents-panel="security" hidden={page !== "security"}>
        <AgentSecurity
          connections={connections}
          blockers={blockers}
          onSaved={(all) => {
            setConnections(all);
            // The app keeps its own list: a later save from a stale copy would undo the change.
            workbench().setConnections(all);
          }}
          onError={setError}
        />
      </div>
    </div>
  );
}
