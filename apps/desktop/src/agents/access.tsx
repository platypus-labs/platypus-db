/** What external agents may do on a connection: the choices, and the field of the
 *  connection dialog. The rules mirror `SavedConnection::effective_agent_access`. */
import type { AgentAccess, SavedConnection } from "../api";
import { isProduction } from "../connections";
import { t, msg } from "../i18n";

export const AGENT_ACCESS: { value: AgentAccess; label: string }[] = [
  { value: "off", label: msg("Off") },
  { value: "read_only", label: msg("Read only") },
  { value: "read_write", label: msg("Read and write") },
];

export const agentAccessLabel = (access: AgentAccess) =>
  t(AGENT_ACCESS.find((a) => a.value === access)?.label ?? access);

/** Production and read-only connections never let agents write. */
const capped = (c: SavedConnection) => isProduction(c) || c.read_only;

/** What agents may do in fact: nothing where the data is not exported. */
export function effectiveAgentAccess(c: SavedConnection): AgentAccess {
  if (c.restrictions?.no_export) return "off";
  const access = c.agent_access ?? "read_only";
  return access === "read_write" && capped(c) ? "read_only" : access;
}

/** The connection with the access agents get in fact, as saved. */
export const withEffectiveAgentAccess = (c: SavedConnection): SavedConnection => ({
  ...c,
  agent_access: effectiveAgentAccess(c),
});

/** The "Agent access" field of the connection dialog. */
export function AgentAccessField({
  conn,
  onChange,
}: {
  conn: SavedConnection;
  onChange: (access: AgentAccess) => void;
}) {
  const limit = capped(conn);
  const kept = !!conn.restrictions?.no_export;
  return (
    <label className="field">
      <span>{t("Agent access")}</span>
      <select
        value={effectiveAgentAccess(conn)}
        onChange={(e) => onChange(e.target.value as AgentAccess)}
        data-field="agent_access"
        disabled={kept}
        title={t("What agents such as Claude Code or Codex may do through PlatypusDB's MCP server")}
      >
        {AGENT_ACCESS.map((a) => (
          <option key={a.value} value={a.value} disabled={limit && a.value === "read_write"}>
            {t(a.label)}
          </option>
        ))}
      </select>
      {(limit || kept) && (
        <span className="muted small" data-field="agent_access_note">
          {t(
            kept
              ? "Agents do not see a connection whose data is not exported."
              : isProduction(conn)
                ? "Production connections are read-only for agents."
                : "Read-only connections are read-only for agents too.",
          )}
        </span>
      )}
    </label>
  );
}
