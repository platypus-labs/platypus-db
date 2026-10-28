import { useEffect, useState } from "react";
import { api, toCommandError, type AgentSetup, type SavedConnection } from "../api";
import { isProduction } from "../connections";
import { agentAccessLabel, effectiveAgentAccess } from "./access";
import { t, tNodes } from "../i18n";

/** A command to paste somewhere else, with its Copy button. */
function CopyBlock({ label, text, field }: { label: string; text: string; field: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () =>
    navigator.clipboard.writeText(text).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      },
      () => setCopied(false),
    );
  return (
    <div className="agent-command">
      <div className="row tight">
        <strong className="small">{label}</strong>
        <span className="spacer" />
        <button type="button" onClick={() => void copy()} data-copy={field}>
          {t(copied ? "Copied" : "Copy")}
        </button>
      </div>
      <pre className="sql-preview" data-field={field}>
        {text}
      </pre>
    </div>
  );
}

/** The Agents page of the Settings: what agents can do, how to connect them, and the
 *  access each connection gives them. */
export function AgentsPanel() {
  const [setup, setSetup] = useState<AgentSetup | null>(null);
  const [connections, setConnections] = useState<SavedConnection[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const failed = (e: unknown) => setError(toCommandError(e).message);
    api.agentSetup().then(setSetup, failed);
    api.listConnections().then(setConnections, failed);
  }, []);

  return (
    <div className="agents-panel">
      <p className="muted small">
        {tNodes(
          "Agents such as Claude Code, Codex or Cursor can use your saved connections through PlatypusDB's MCP server: the app itself, started with {mcp}. The agent starts it by itself, without a window: PlatypusDB does not need to be open. Agents read the schema and run queries with the access each connection gives them; passwords stay in the keychain and never reach them. Their queries are in the history, with the agent's name.",
          { mcp: <code>--mcp</code> },
        )}
      </p>
      {error && <div className="notice error small">{error}</div>}
      {setup && (
        <>
          <CopyBlock label="Claude Code" text={setup.claude_command} field="claude_command" />
          <CopyBlock label="Codex (~/.codex/config.toml)" text={setup.codex_config} field="codex_config" />
        </>
      )}
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
            return (
              <tr key={c.id} data-connection={c.name}>
                <td>{c.name}</td>
                <td className={access === "off" ? "muted" : undefined}>
                  {agentAccessLabel(access)}
                  {isProduction(c) && access !== "off" && <span className="muted small"> · {t("production")}</span>}
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
    </div>
  );
}
