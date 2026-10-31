import { useEffect, useState } from "react";
import { api, toCommandError, type AgentClient, type AgentInstallState, type AgentSetup } from "../api";
import { ask } from "../ui/ask";
import { msg, t, tNodes } from "../i18n";
import { Icon } from "../ui/Icon";
import { ClientLogo } from "./clientLogos";
import { CopyBlock } from "./CopyBlock";

function stateLabel(state: AgentInstallState): string {
  switch (state) {
    case "installed":
      return t("Installed");
    case "different":
      return t("Installed, with another command");
    case "unreadable":
      return t("The file cannot be read");
    default:
      return t("Not installed");
  }
}

const KINDS: Record<string, string> = {
  claude_desktop: msg("Desktop app"),
  claude_code: msg("Command line"),
  codex: msg("Command line"),
  gemini_cli: msg("Command line"),
};

/** The kind of client: an app, a command line tool, or (by default) an editor. */
function kindLabel(id: string): string {
  return t(KINDS[id] ?? msg("Editor"));
}

function stateDot(state: AgentInstallState): string {
  return state === "installed" ? "ok" : state === "different" ? "warn" : "";
}

/** What to paste by hand: Claude Code's command, otherwise the entry in the client's own format. */
function manualText(client: AgentClient, setup: AgentSetup | null): { text: string; field: string; fallback: boolean } {
  if (client.id === "claude_code") {
    if (setup) return { text: setup.claude_command, field: "claude_command", fallback: false };
    return { text: client.snippet, field: `snippet_${client.id}`, fallback: true };
  }
  return { text: client.snippet, field: `snippet_${client.id}`, fallback: false };
}

/** The clients PlatypusDB can be installed in: each with its file and an Install button
 *  that writes the server there after a backup, and the entry to copy by hand. */
export function AgentClients() {
  const [clients, setClients] = useState<AgentClient[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [setup, setSetup] = useState<AgentSetup | null>(null);
  const [setupFailed, setSetupFailed] = useState(false);

  const load = () =>
    api.agentClients().then(
      (list) => {
        setClients(list);
        setError(null);
        setChosen((c) => c ?? list[0]?.id ?? null);
      },
      (e: unknown) => setError(toCommandError(e).message),
    );
  useEffect(() => {
    void load();
    api.agentSetup().then(setSetup, () => setSetupFailed(true));
  }, []);

  const install = async (client: AgentClient) => {
    const message = client.exists
      ? t("Write the PlatypusDB server in {path}? A copy of the file as it is now is saved beside it first.", {
          path: client.path,
        })
      : t("Create {path} with the PlatypusDB server?", { path: client.path });
    if (!(await ask(message, { confirm: t("Install") }))) return;
    try {
      const done = await api.installAgentClient(client.id);
      setError(null);
      setNotice(
        done.backup
          ? t("PlatypusDB is in {client}: restart it to load the server. Backup: {backup}", {
              client: client.name,
              backup: done.backup,
            })
          : t("PlatypusDB is in {client}: restart it to load the server.", { client: client.name }),
      );
      await load();
    } catch (e) {
      setNotice(null);
      setError(toCommandError(e).message);
    }
  };

  const shown = clients.find((c) => c.id === chosen);
  const manual = shown && manualText(shown, setup);
  return (
    <div className="agent-clients">
      {error && <div className="notice error small">{error}</div>}
      {notice && (
        <div className="notice small" data-notice="installed">
          {notice}
        </div>
      )}
      <div className="client-grid">
        {clients.map((c) => {
          const done = c.state === "installed";
          return (
            <div
              key={c.id}
              data-client={c.id}
              data-state={c.state}
              className={`client-card${c.id === chosen ? " selected" : ""}`}
              role="button"
              tabIndex={0}
              aria-pressed={c.id === chosen}
              onClick={() => setChosen(c.id)}
              onKeyDown={(e) => {
                if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
                  e.preventDefault();
                  setChosen(c.id);
                }
              }}
            >
              <ClientLogo id={c.id} />
              <span className="client-name">{c.name}</span>
              <span className="client-kind">{kindLabel(c.id)}</span>
              <div className="client-foot">
                <span className={`client-state${done ? " ok" : ""}`}>
                  <span className={`agent-dot ${stateDot(c.state)}`} aria-hidden />
                  {stateLabel(c.state)}
                </span>
                <button
                  type="button"
                  data-install={c.id}
                  disabled={done || c.state === "unreadable"}
                  onClick={(e) => {
                    e.stopPropagation();
                    void install(c);
                  }}
                >
                  {done && <Icon name="commit" size={12} />}
                  {done ? t("Done") : c.state === "different" ? t("Update") : t("Install")}
                </button>
              </div>
            </div>
          );
        })}
      </div>
      {shown && manual && (
        <section className="client-detail" data-detail={shown.id}>
          <header>
            <ClientLogo id={shown.id} size={28} />
            <strong>{shown.name}</strong>
            <span className="muted small">
              {shown.exists
                ? t("Install writes the server in the file and first saves a copy of it beside it")
                : t("Install creates the file with the server")}
            </span>
          </header>
          <div className="client-detail-body">
            <div className="client-detail-col">
              <div>
                <div className="client-label">{t("Configuration file")}</div>
                <div className="small agent-client-path">
                  <code>{shown.path}</code>
                </div>
                {!shown.exists && <div className="muted small">{t("Not there yet: Install creates it.")}</div>}
              </div>
              <div>
                <div className="client-label">{t("After installing")}</div>
                <ol className="client-steps small muted">
                  <li>{t("Restart {client}.", { client: shown.name })}</li>
                  <li>
                    {tNodes("PlatypusDB's tools appear among those of the server {name}.", {
                      name: <code>platypus</code>,
                    })}
                  </li>
                  <li>{t("Choose in Security what each connection allows.")}</li>
                </ol>
              </div>
            </div>
            <div className="client-detail-col">
              {manual.fallback && setupFailed && (
                <div className="muted small">{t("The command is not available here: use this entry instead.")}</div>
              )}
              <CopyBlock label={t("By hand")} text={manual.text} field={manual.field} />
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
