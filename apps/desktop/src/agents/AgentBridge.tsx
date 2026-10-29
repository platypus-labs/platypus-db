import { useEffect, useState } from "react";
import { api, toCommandError, type BridgeStatus } from "../api";
import { t } from "../i18n";
import { setStored } from "../settings";
import { ask } from "../ui/ask";
import { CopyBlock } from "./CopyBlock";

/** The settings of the bridge, read by the backend (`agent/commands.rs`). */
const ENABLED_KEY = "agents.bridge";
const PORT_KEY = "agents.bridge_port";

/** `claude mcp add` for the bridge's HTTP endpoint, with its token. */
export function claudeHttpCommand(url: string, token: string): string {
  return `claude mcp add --transport http platypus ${url} --header "Authorization: Bearer ${token}"`;
}

/** The bridge with the open app, in Settings › Agents › Setup: on or off, its port, its
 *  address and token for MCP clients over HTTP, and a new token. */
export function AgentBridge() {
  const [status, setStatus] = useState<BridgeStatus | null>(null);
  const [port, setPort] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const shown = (s: BridgeStatus) => {
    setStatus(s);
    setPort(String(s.port));
  };
  useEffect(() => {
    api.agentBridge().then(shown, (e: unknown) => setError(toCommandError(e).message));
  }, []);

  /** Stores `key` and starts the bridge again with it. */
  const change = async (key: string, value: boolean | number | null) => {
    setBusy(true);
    setError(null);
    try {
      await setStored(key, value);
      shown(await api.restartAgentBridge());
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  };
  const applyPort = () => {
    const n = Number(port);
    if (!status || n === status.port) return;
    if (!Number.isInteger(n) || n < 1 || n > 65535) {
      setError(t("The port is a number from 1 to 65535."));
      return;
    }
    void change(PORT_KEY, n);
  };
  const regenerate = async () => {
    if (!(await ask(t("Make a new token? Clients set up with the current one stop working until you set them up again."), { confirm: t("New token") }))) return;
    setBusy(true);
    try {
      shown(await api.regenerateAgentToken());
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  };

  const enabled = status?.enabled ?? true;
  return (
    <section className="agent-bridge" data-section="bridge">
      <h3>{t("Bridge with the open app")}</h3>
      <p className="muted small">
        {t(
          "While PlatypusDB is open, agents reach it on a port of this computer only, with a token: they see the sessions open here, show in the top bar while connected, and ask here before a change. MCP clients that speak HTTP can also connect to it directly.",
        )}
      </p>
      <div className="row tight">
        <label className="check">
          <input
            type="checkbox"
            checked={enabled}
            disabled={busy || !status}
            data-field="bridge_enabled"
            onChange={(e) => void change(ENABLED_KEY, e.target.checked ? null : false)}
          />
          {t("Open the bridge")}
        </label>
        <span className="spacer" />
        <label className="agent-bridge-port small">
          {t("Port")}
          <input
            type="number"
            min={1}
            max={65535}
            value={port}
            disabled={busy || !status}
            data-field="bridge_port"
            onChange={(e) => setPort(e.target.value)}
            onBlur={applyPort}
            onKeyDown={(e) => e.key === "Enter" && applyPort()}
          />
        </label>
      </div>
      {error && <div className="notice error small">{error}</div>}
      {status?.error && enabled && (
        <div className="notice error small" data-field="bridge_error">
          {status.error}
        </div>
      )}
      {status?.url && (
        <>
          <div className="notice ok small" data-field="bridge_url">
            {t("Open at {url}", { url: status.url })}
          </div>
          <CopyBlock label={t("Claude Code, over HTTP")} text={claudeHttpCommand(status.url, status.token)} field="claude_http_command" />
          <CopyBlock label={t("Token (Authorization: Bearer …)")} text={status.token} field="bridge_token" />
          <div className="row tight">
            <span className="spacer" />
            <button type="button" disabled={busy} onClick={() => void regenerate()} data-action="regenerate_token">
              {t("New token…")}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
