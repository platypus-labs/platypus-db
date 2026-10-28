import { useEffect, useState } from "react";
import { api, toCommandError, type AgentClient, type AgentInstallState } from "../api";
import { ask } from "../ui/ask";
import { t } from "../i18n";
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

/** The clients PlatypusDB can be installed in: each with its file and an Install button
 *  that writes the server there after a backup, and the entry to copy by hand. */
export function AgentClients() {
  const [clients, setClients] = useState<AgentClient[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = () =>
    api.agentClients().then(
      (list) => {
        setClients(list);
        setChosen((c) => c ?? list[0]?.id ?? null);
      },
      (e: unknown) => setError(toCommandError(e).message),
    );
  useEffect(() => void load(), []);

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
  return (
    <div className="agent-clients">
      {error && <div className="notice error small">{error}</div>}
      {notice && (
        <div className="notice small" data-notice="installed">
          {notice}
        </div>
      )}
      <table className="agents-connections">
        <thead>
          <tr>
            <th>{t("Client")}</th>
            <th>{t("Configuration file")}</th>
            <th>{t("State")}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {clients.map((c) => (
            <tr
              key={c.id}
              data-client={c.id}
              data-state={c.state}
              className={c.id === chosen ? "selected" : undefined}
              onClick={() => setChosen(c.id)}
            >
              <td>{c.name}</td>
              <td className="small agent-client-path">
                <code>{c.path}</code>
                {!c.exists && <span className="muted"> · {t("not there yet")}</span>}
              </td>
              <td className={c.state === "installed" ? undefined : "muted"}>{stateLabel(c.state)}</td>
              <td>
                <button
                  type="button"
                  data-install={c.id}
                  disabled={c.state === "installed" || c.state === "unreadable"}
                  onClick={(e) => {
                    e.stopPropagation();
                    void install(c);
                  }}
                >
                  {c.state === "different" ? t("Update") : t("Install")}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {shown && (
        <CopyBlock
          label={t("By hand, in {path}", { path: shown.path })}
          text={shown.snippet}
          field={`snippet_${shown.id}`}
        />
      )}
    </div>
  );
}
