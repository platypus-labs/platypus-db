import { useEffect, useRef, useState } from "react";
import { api, toCommandError, type SavedConnection } from "./api";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { QueryTab, type TabModel } from "./components/QueryTab";
import { Navigator } from "./components/Navigator";
import { basename, loadWorkspace, saveWorkspace } from "./workspace";

type Dialog = { kind: "connection"; connection: SavedConnection; existing: boolean } | null;

interface PasswordRequest {
  connection: SavedConnection;
  resolve: (password: string | null) => void;
}

let nextTabKey = 1;

function newTab(
  connectionId: string,
  sql = "",
  filePath: string | null = null,
  autorun = false,
  title: string | null = null,
): TabModel {
  return { key: nextTabKey++, connectionId, sql, filePath, savedSql: filePath ? sql : null, autorun, title };
}

export function App() {
  const [connections, setConnections] = useState<SavedConnection[] | null>(null);
  const [storeError, setStoreError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest | null>(null);
  const [tabs, setTabs] = useState<TabModel[]>([]);
  const [activeTab, setActiveTab] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  /** Open sessions by tab key; opened on a tab's first run. */
  const sessions = useRef(new Map<number, Promise<number | null>>());
  /** Navigator sessions by connection id, separate from the editors' so browsing never
   *  waits behind a running query. */
  const navSessions = useRef(new Map<string, Promise<number | null>>());
  const restored = useRef(false);

  useEffect(() => {
    api.listConnections().then(setConnections, (e: unknown) => {
      setConnections([]);
      setStoreError(toCommandError(e).message);
    });
    api.appInfo().then((i) => setVersion(i.version), console.error);
  }, []);

  // Restore the previous workspace once connections are known.
  useEffect(() => {
    if (!connections || restored.current) return;
    restored.current = true;
    const saved = loadWorkspace();
    if (!saved) return;
    const known = new Set(connections.map((c) => c.id));
    const restoredTabs = saved.tabs
      .filter((t) => known.has(t.connectionId))
      .map((t) => ({ ...newTab(t.connectionId, t.sql, t.filePath, false, t.title ?? null), savedSql: t.savedSql }));
    setTabs(restoredTabs);
    setActiveTab(restoredTabs[Math.min(saved.active, restoredTabs.length - 1)]?.key ?? null);
  }, [connections]);

  useEffect(() => {
    if (!restored.current) return;
    saveWorkspace({
      tabs: tabs.map(({ connectionId, sql, filePath, savedSql, title }) => ({
        connectionId,
        sql,
        filePath,
        savedSql,
        title,
      })),
      active: Math.max(0, tabs.findIndex((t) => t.key === activeTab)),
    });
  }, [tabs, activeTab]);

  const connectionById = (id: string) => connections?.find((c) => c.id === id);

  async function newConnection() {
    setDialog({ kind: "connection", connection: await api.defaultConnection("postgres"), existing: false });
  }

  async function deleteConnection(c: SavedConnection) {
    if (!window.confirm(`Delete the connection "${c.name}"?`)) return;
    try {
      setConnections(await api.deleteConnection(c.id));
      for (const t of tabs.filter((t) => t.connectionId === c.id)) closeTab(t);
      disconnectNavigator(c);
    } catch (e) {
      setToast(toCommandError(e).message);
    }
  }

  function addTab(tab: TabModel) {
    setTabs((all) => [...all, tab]);
    setActiveTab(tab.key);
  }

  function askPassword(connection: SavedConnection) {
    return new Promise<string | null>((resolve) => setPasswordRequest({ connection, resolve }));
  }

  /** Opens a session, asking for the password when it is not saved; `null` on failure. */
  async function connect(connection: SavedConnection): Promise<number | null> {
    let password: string | undefined;
    if (!connection.save_password && connection.driver !== "sqlite") {
      const entered = await askPassword(connection);
      if (entered === null) return null;
      password = entered;
    }
    try {
      const { session_id } = await api.openSession(connection.id, password);
      return session_id;
    } catch (e) {
      setToast(`${connection.name}: ${toCommandError(e).message}`);
      return null;
    }
  }

  /** Caches a session promise; failed or cancelled attempts are retried on next use. */
  function cached<K>(map: Map<K, Promise<number | null>>, key: K, open: () => Promise<number | null>) {
    const existing = map.get(key);
    if (existing) return existing;
    const opening = open();
    map.set(key, opening);
    void opening.then((id) => id === null && map.delete(key));
    return opening;
  }

  function ensureSession(tab: TabModel): Promise<number | null> {
    return cached(sessions.current, tab.key, async () => {
      const connection = connectionById(tab.connectionId);
      return connection ? connect(connection) : null;
    });
  }

  function navigatorSession(connection: SavedConnection) {
    return cached(navSessions.current, connection.id, () => connect(connection));
  }

  function disconnectNavigator(connection: SavedConnection) {
    const session = navSessions.current.get(connection.id);
    navSessions.current.delete(connection.id);
    void session?.then((id) => {
      if (id !== null) void api.closeSession(id);
    });
  }

  function closeTab(tab: TabModel) {
    const session = sessions.current.get(tab.key);
    sessions.current.delete(tab.key);
    void session?.then((id) => {
      if (id !== null) void api.closeSession(id);
    });
    setTabs((all) => {
      const rest = all.filter((t) => t.key !== tab.key);
      setActiveTab((active) => (active === tab.key ? (rest.at(-1)?.key ?? null) : active));
      return rest;
    });
  }

  function patchTab(key: number, patch: Partial<TabModel>) {
    setTabs((all) => all.map((t) => (t.key === key ? { ...t, ...patch } : t)));
  }

  function tabTitle(tab: TabModel) {
    const name = connectionById(tab.connectionId)?.name ?? "?";
    if (!tab.filePath) return tab.title ? `${tab.title} — ${name}` : name;
    const dirty = tab.sql !== tab.savedSql ? " •" : "";
    return `${basename(tab.filePath)}${dirty} — ${name}`;
  }

  return (
    <div className="shell">
      <Navigator
        connections={connections ?? []}
        error={storeError}
        onNew={() => void newConnection()}
        onEdit={(c) => setDialog({ kind: "connection", connection: c, existing: true })}
        onDelete={(c) => void deleteConnection(c)}
        onOpenTab={(c, sql, autorun, title) => addTab(newTab(c.id, sql, null, autorun, title))}
        session={navigatorSession}
        disconnect={disconnectNavigator}
        onError={setToast}
      />
      <main className="workspace">
        {tabs.length > 0 && (
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.key}
                className={`tab${t.key === activeTab ? " active" : ""}`}
                style={{ borderTopColor: connectionById(t.connectionId)?.color ?? "transparent" }}
                onClick={() => setActiveTab(t.key)}
                onAuxClick={(e) => e.button === 1 && closeTab(t)}
              >
                <span>{tabTitle(t)}</span>
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
        {tabs.map((t) => {
          const connection = connectionById(t.connectionId);
          return (
            connection && (
              <QueryTab
                key={t.key}
                tab={t}
                connection={connection}
                active={t.key === activeTab}
                ensureSession={() => ensureSession(t)}
                onChange={(patch) => patchTab(t.key, patch)}
                onOpenFile={(path, sql) => addTab(newTab(t.connectionId, sql, path))}
                onError={setToast}
              />
            )
          );
        })}
        {tabs.length === 0 && (
          <div className="welcome">
            <h1>PlatypusDB</h1>
            <p className="muted">
              {connections?.length === 0
                ? "Add a connection to get started."
                : "Double-click a connection to open a SQL editor, or expand it to browse."}
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
      {passwordRequest && (
        <PasswordPrompt
          connection={passwordRequest.connection}
          onDone={(password) => {
            passwordRequest.resolve(password);
            setPasswordRequest(null);
          }}
        />
      )}
    </div>
  );
}

function PasswordPrompt({
  connection,
  onDone,
}: {
  connection: SavedConnection;
  onDone: (password: string | null) => void;
}) {
  const [password, setPassword] = useState("");
  return (
    <div className="modal-backdrop">
      <form
        className="modal small"
        onSubmit={(e) => {
          e.preventDefault();
          onDone(password);
        }}
        onKeyDown={(e) => e.key === "Escape" && onDone(null)}
      >
        <h2>Password for {connection.name}</h2>
        <label className="field">
          <span>
            {connection.user}@{connection.host}
          </span>
          <input autoFocus type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </label>
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={() => onDone(null)}>
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
