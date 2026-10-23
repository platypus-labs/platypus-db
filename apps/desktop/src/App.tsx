import { useEffect, useState } from "react";
import { api, toCommandError, type SavedConnection, type ServerInfo } from "./api";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { QueryTab } from "./components/QueryTab";
import { Sidebar } from "./components/Sidebar";

interface Tab {
  key: number;
  connection: SavedConnection;
  sessionId: number;
  server: ServerInfo;
}

type Dialog =
  | { kind: "connection"; connection: SavedConnection; existing: boolean }
  | { kind: "password"; connection: SavedConnection }
  | null;

let nextTabKey = 1;

export function App() {
  const [connections, setConnections] = useState<SavedConnection[]>([]);
  const [storeError, setStoreError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTab, setActiveTab] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [version, setVersion] = useState("");

  useEffect(() => {
    api.listConnections().then(setConnections, (e: unknown) => setStoreError(toCommandError(e).message));
    api.appInfo().then((i) => setVersion(i.version), console.error);
  }, []);

  async function newConnection() {
    setDialog({ kind: "connection", connection: await api.defaultConnection("postgres"), existing: false });
  }

  async function deleteConnection(c: SavedConnection) {
    if (!window.confirm(`Delete the connection "${c.name}"?`)) return;
    try {
      setConnections(await api.deleteConnection(c.id));
    } catch (e) {
      setToast(toCommandError(e).message);
    }
  }

  async function openTab(connection: SavedConnection, password?: string) {
    if (!connection.save_password && password === undefined) {
      setDialog({ kind: "password", connection });
      return;
    }
    setToast(null);
    try {
      const { session_id, server } = await api.openSession(connection.id, password);
      const key = nextTabKey++;
      setTabs((t) => [...t, { key, connection, sessionId: session_id, server }]);
      setActiveTab(key);
    } catch (e) {
      setToast(`${connection.name}: ${toCommandError(e).message}`);
    }
  }

  function closeTab(tab: Tab) {
    void api.closeSession(tab.sessionId);
    setTabs((all) => {
      const rest = all.filter((t) => t.key !== tab.key);
      if (activeTab === tab.key) setActiveTab(rest.at(-1)?.key ?? null);
      return rest;
    });
  }

  return (
    <div className="shell">
      <Sidebar
        connections={connections}
        error={storeError}
        onNew={() => void newConnection()}
        onEdit={(c) => setDialog({ kind: "connection", connection: c, existing: true })}
        onDelete={(c) => void deleteConnection(c)}
        onOpen={(c) => void openTab(c)}
      />
      <main className="workspace">
        {tabs.length > 0 && (
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.key}
                className={`tab${t.key === activeTab ? " active" : ""}`}
                style={{ borderTopColor: t.connection.color ?? "transparent" }}
                onClick={() => setActiveTab(t.key)}
                onAuxClick={(e) => e.button === 1 && closeTab(t)}
                title={`${t.server.product} ${t.server.version}${t.server.encrypted ? " · TLS" : ""}`}
              >
                <span>{t.connection.name}</span>
                <button
                  className="icon"
                  onClick={(e) => {
                    e.stopPropagation();
                    closeTab(t);
                  }}
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        )}
        {toast && (
          <div className="notice error toast" onClick={() => setToast(null)}>
            {toast}
          </div>
        )}
        {tabs.map((t) => (
          <QueryTab
            key={t.key}
            sessionId={t.sessionId}
            connection={t.connection}
            active={t.key === activeTab}
          />
        ))}
        {tabs.length === 0 && (
          <div className="welcome">
            <h1>PlatypusDB</h1>
            <p className="muted">
              {connections.length === 0
                ? "Add a connection to get started."
                : "Double-click a connection to open a SQL editor."}
            </p>
            <p className="muted small">v{version}</p>
          </div>
        )}
      </main>

      {dialog?.kind === "connection" && (
        <ConnectionDialog
          initial={dialog.connection}
          existing={dialog.existing}
          onClose={() => setDialog(null)}
          onSaved={(all) => {
            setConnections(all);
            setDialog(null);
          }}
        />
      )}
      {dialog?.kind === "password" && (
        <PasswordPrompt
          connection={dialog.connection}
          onCancel={() => setDialog(null)}
          onSubmit={(password) => {
            setDialog(null);
            void openTab(dialog.connection, password);
          }}
        />
      )}
    </div>
  );
}

function PasswordPrompt({
  connection,
  onCancel,
  onSubmit,
}: {
  connection: SavedConnection;
  onCancel: () => void;
  onSubmit: (password: string) => void;
}) {
  const [password, setPassword] = useState("");
  return (
    <div className="modal-backdrop">
      <form
        className="modal small"
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit(password);
        }}
        onKeyDown={(e) => e.key === "Escape" && onCancel()}
      >
        <h2>Password for {connection.name}</h2>
        <label className="field">
          <span>
            {connection.user}@{connection.host}
          </span>
          <input
            autoFocus
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </label>
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="primary">
            Connect
          </button>
        </div>
      </form>
    </div>
  );
}
