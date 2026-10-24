import { type ComponentType, Fragment, type ReactNode, useEffect, useRef, useState } from "react";
import { dispatchKey, registerAction } from "./actions";
import { api, type Capabilities, type CompletionCatalog, toCommandError, type SavedConnection } from "./api";
import { onNavigatorRefresh } from "./navigatorEvents";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { ImportDialog } from "./components/ImportDialog";
import { connectionColor } from "./connections";
import { Navigator } from "./components/Navigator";
import { SettingsDialog } from "./components/SettingsDialog";
import { UpdateBanner } from "./components/UpdateBanner";
import { CommandPalette } from "./shell/CommandPalette";
import "./tabs";
import "./objects/actions";
import { type TabKindName, type TabModel, type TabProps, createTab, tabKind } from "./tabs/registry";
import { provideWorkbench, workbench } from "./workbench";
import { type PersistedTab, loadWorkspace, saveWorkspace } from "./workspace";

type Dialog = { kind: "connection"; connection: SavedConnection; existing: boolean } | { kind: "import" } | null;

interface PasswordRequest {
  connection: SavedConnection;
  resolve: (password: string | null) => void;
}

export function App() {
  const [connections, setConnections] = useState<SavedConnection[] | null>(null);
  const [storeError, setStoreError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest | null>(null);
  const [tabs, setTabs] = useState<TabModel[]>([]);
  const [activeTab, setActiveTab] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  /** Closed tabs as they would be saved, newest last, for Ctrl+Shift+T. */
  const closedTabs = useRef<PersistedTab[]>([]);
  const tabBar = useRef<HTMLDivElement>(null);
  /** Dialogs opened by actions through `workbench().openDialog`. */
  const [dialogs, setDialogs] = useState<{ id: number; node: ReactNode }[]>([]);
  const nextDialog = useRef(1);
  /** What each open session's driver supports. */
  const capabilities = useRef(new Map<number, Capabilities>());
  const [version, setVersion] = useState("");
  /** Open sessions by tab key; opened on a tab's first run. */
  const sessions = useRef(new Map<number, Promise<number | null>>());
  /** Navigator sessions by `dbKey`, separate from the editors' so browsing never waits
   *  behind a running query. */
  const navSessions = useRef(new Map<string, Promise<number | null>>());
  /** Passwords typed this run, in memory only, so a second session does not ask again. */
  const passwords = useRef(new Map<string, string>());
  const [catalogs, setCatalogs] = useState<Record<string, CompletionCatalog>>({});
  const catalogLoads = useRef(new Map<string, Promise<unknown>>());
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
    const restoredTabs = saved.tabs.flatMap((t) => {
      // Tabs saved before tab kinds existed are SQL editors.
      const kind = tabKind(t.kind ?? "query");
      if (!kind || !known.has(t.connectionId)) return [];
      return [createTab(kind.kind, t.connectionId, kind.restore(t))];
    });
    setTabs(restoredTabs);
    setActiveTab(restoredTabs[Math.min(saved.active, restoredTabs.length - 1)]?.key ?? null);
  }, [connections]);

  useEffect(() => {
    if (!restored.current) return;
    const kept = tabs.flatMap((tab) => {
      const state = tabKind(tab.kind)?.persist(tab);
      const saved: PersistedTab | null = state ? { ...state, kind: tab.kind, connectionId: tab.connectionId } : null;
      return saved ? [{ key: tab.key, saved }] : [];
    });
    saveWorkspace({
      tabs: kept.map((k) => k.saved),
      active: Math.max(0, kept.findIndex((k) => k.key === activeTab)),
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
      for (const t of tabs.filter((t) => t.connectionId === c.id)) closeTab(t, true);
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

  /** Opens a session, asking for the password when it is not saved; `null` on failure.
   *  `database` opens another database of the server than the saved one. */
  async function connect(connection: SavedConnection, database?: string | null): Promise<number | null> {
    let password: string | undefined;
    if (!connection.save_password && connection.driver !== "sqlite") {
      password = passwords.current.get(connection.id);
      if (password === undefined) {
        const entered = await askPassword(connection);
        if (entered === null) return null;
        password = entered;
      }
    }
    try {
      const session = await api.openSession(connection.id, password, database);
      if (password !== undefined) passwords.current.set(connection.id, password);
      capabilities.current.set(session.session_id, session.capabilities);
      return session.session_id;
    } catch (e) {
      passwords.current.delete(connection.id);
      setToast(`${connection.name}: ${toCommandError(e).message}`);
      return null;
    }
  }

  /** Loads (or with `force` reloads) the tables and columns the editors complete. */
  function requestCatalog(connection: SavedConnection, force = false, database?: string | null) {
    const key = dbKey(connection.id, database);
    if (!force && catalogLoads.current.has(key)) return;
    const loading = (async () => {
      const sessionId = await navigatorSession(connection, database);
      if (sessionId === null) return catalogLoads.current.delete(key);
      try {
        const catalog = await api.completionCatalog(sessionId);
        setCatalogs((all) => ({ ...all, [key]: catalog }));
      } catch (e) {
        catalogLoads.current.delete(key);
        console.warn("autocompletion catalog", e);
      }
    })();
    catalogLoads.current.set(key, loading);
  }

  // DDL run from the navigator changes what autocompletion should offer.
  useEffect(
    () =>
      onNavigatorRefresh(({ connectionId, database }) => {
        const connection = shell.current.connections?.find((c) => c.id === connectionId);
        if (connection && catalogLoads.current.has(dbKey(connectionId, database))) {
          requestCatalog(connection, true, database);
        }
      }),
    [],
  );

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
      return connection ? connect(connection, tab.database) : null;
    });
  }

  function navigatorSession(connection: SavedConnection, database?: string | null) {
    return cached(navSessions.current, dbKey(connection.id, database), () => connect(connection, database));
  }

  /** Closes the navigator sessions of every database of the connection. */
  function disconnectNavigator(connection: SavedConnection) {
    passwords.current.delete(connection.id);
    for (const key of [...catalogLoads.current.keys()]) {
      if (key.split("\u0000")[0] === connection.id) catalogLoads.current.delete(key);
    }
    for (const [key, session] of [...navSessions.current]) {
      if (key.split("\u0000")[0] !== connection.id) continue;
      navSessions.current.delete(key);
      void session.then((id) => {
        if (id !== null) void api.closeSession(id);
      });
    }
  }

  function closeTab(tab: TabModel, confirmed = false) {
    const question = tab.unsaved
      ? "This tab has unsaved changes. Close it and discard them?"
      : tab.txOpen
        ? "This tab has an open transaction. Close it and roll back the uncommitted changes?"
        : null;
    if (question && !confirmed && !window.confirm(question)) return;
    const state = tabKind(tab.kind)?.persist(tab);
    if (state) {
      closedTabs.current.push({ ...state, kind: tab.kind, connectionId: tab.connectionId });
      if (closedTabs.current.length > CLOSED_TABS) closedTabs.current.shift();
    }
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

  // What the shell's actions and the workbench need, refreshed every render so they
  // always see the latest state while being registered once.
  const reopenTab = () => {
    const saved = closedTabs.current.pop();
    const kind = saved && tabKind((saved.kind ?? "query") as TabKindName);
    if (!saved || !kind || !connectionById(saved.connectionId)) return;
    addTab(createTab(kind.kind, saved.connectionId, kind.restore(saved)));
  };
  const openPalette = () => setPaletteOpen(true);
  const shell = useRef({ tabs, activeTab, connections, addTab, closeTab, setActiveTab, navigatorSession, reopenTab, openPalette });
  shell.current = { tabs, activeTab, connections, addTab, closeTab, setActiveTab, navigatorSession, reopenTab, openPalette };

  // The active tab stays in view when tabs overflow the bar.
  useEffect(() => {
    tabBar.current?.querySelector(".tab.active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeTab, tabs.length]);

  provideWorkbench({
    connections: () => shell.current.connections ?? [],
    setConnections,
    openTab: (kind, connectionId, init) => shell.current.addTab(createTab(kind, connectionId, init)),
    sideSession: (connection, database) => shell.current.navigatorSession(connection, database),
    capabilities: (sessionId) => capabilities.current.get(sessionId),
    showError: setToast,
    openSettings: () => setSettingsOpen(true),
    openDialog: (render) => {
      const id = nextDialog.current++;
      const close = () => setDialogs((all) => all.filter((d) => d.id !== id));
      setDialogs((all) => [...all, { id, node: render(close) }]);
    },
  });

  useEffect(() => registerShellActions(shell), []);

  // Global shortcuts, captured before the editor sees them.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      dispatchKey("global", undefined, e, (err) => setToast(toCommandError(err).message));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  function patchTab(key: number, patch: Partial<TabModel>) {
    // The patch comes from the tab's own component, so it matches the tab's kind.
    setTabs((all) => all.map((t) => (t.key === key ? ({ ...t, ...patch } as TabModel) : t)));
  }

  function tabTitle(tab: TabModel) {
    const connection = connectionById(tab.connectionId);
    // Another database than the saved one is part of the name.
    const name =
      tab.database && tab.database !== connection?.database
        ? `${connection?.name ?? "?"}/${tab.database}`
        : (connection?.name ?? "?");
    const label = tabKind(tab.kind)?.label(tab);
    return label ? `${label} — ${name}` : name;
  }

  return (
    <div className="shell">
      <Navigator
        connections={connections ?? []}
        error={storeError}
        onNew={() => void newConnection()}
        onImport={() => setDialog({ kind: "import" })}
        onEdit={(c) => setDialog({ kind: "connection", connection: c, existing: true })}
        onDelete={(c) => void deleteConnection(c)}
        onOpenTab={(c, init) => addTab(createTab("query", c.id, init))}
        session={navigatorSession}
        capabilities={(sessionId) => capabilities.current.get(sessionId)}
        disconnect={disconnectNavigator}
        onRefresh={(c, database) => requestCatalog(c, true, database)}
        onError={setToast}
      />
      <main className="workspace">
        <UpdateBanner />
        {tabs.length > 0 && (
          <div
            className="tabs"
            ref={tabBar}
            // The wheel scrolls the tabs sideways: the bar has nothing to scroll up or down.
            onWheel={(e) => {
              if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
            }}
          >
            {tabs.map((t) => (
              <div
                key={t.key}
                className={`tab${t.key === activeTab ? " active" : ""}`}
                style={{ borderTopColor: connectionColor(connectionById(t.connectionId)) ?? "transparent" }}
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
          const Component = tabKind(t.kind)?.component as ComponentType<TabProps<TabModel>> | undefined;
          return (
            connection &&
            Component && (
              <Component
                key={t.key}
                tab={t}
                connection={connection}
                active={t.key === activeTab}
                ensureSession={() => ensureSession(t)}
                onChange={(patch) => patchTab(t.key, patch)}
                sideSession={() => navigatorSession(connection, t.database)}
                catalog={catalogs[dbKey(connection.id, t.database)] ?? null}
                onNeedCatalog={(force) => requestCatalog(connection, force, t.database)}
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
      {dialog?.kind === "import" && (
        <ImportDialog
          existing={connections ?? []}
          onClose={() => setDialog(null)}
          onImported={(all) => {
            setConnections(all);
            setDialog(null);
          }}
        />
      )}
      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}
      {paletteOpen && (
        <CommandPalette connections={connections ?? []} catalogs={catalogs} onClose={() => setPaletteOpen(false)} />
      )}
      {dialogs.map((d) => (
        <Fragment key={d.id}>{d.node}</Fragment>
      ))}
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

/** Sessions and catalogs are per connection and database; `null` is the saved one. */
function dbKey(connectionId: string, database?: string | null) {
  return `${connectionId}\u0000${database ?? ""}`;
}

interface ShellApi {
  tabs: TabModel[];
  activeTab: number | null;
  connections: SavedConnection[] | null;
  addTab: (tab: TabModel) => void;
  closeTab: (tab: TabModel) => void;
  setActiveTab: (key: number) => void;
  navigatorSession: (connection: SavedConnection, database?: string | null) => Promise<number | null>;
  reopenTab: () => void;
  openPalette: () => void;
}

/** Closed tabs kept for reopening. */
const CLOSED_TABS = 20;

/** Tab and window commands; their shortcuts work everywhere, the editor included. */
function registerShellActions(shell: { current: ShellApi }) {
  const s = () => shell.current;
  const index = () => s().tabs.findIndex((t) => t.key === s().activeTab);
  const move = (delta: number) => {
    const { tabs } = s();
    if (tabs.length > 0) s().setActiveTab(tabs[(index() + delta + tabs.length) % tabs.length]!.key);
  };
  const unregister = [
    registerAction({
      id: "tab.new",
      scope: "global",
      title: "New SQL editor",
      shortcut: "Mod+T",
      run: () => {
        const connectionId = s().tabs[index()]?.connectionId ?? s().connections?.[0]?.id;
        if (connectionId) s().addTab(createTab("query", connectionId));
      },
    }),
    registerAction({
      id: "tab.close",
      scope: "global",
      title: "Close tab",
      shortcut: "Mod+W",
      run: () => {
        const tab = s().tabs[index()];
        if (tab) s().closeTab(tab);
      },
    }),
    registerAction({
      id: "tab.next",
      scope: "global",
      title: "Next tab",
      shortcut: ["Mod+Tab", "Mod+PageDown"],
      run: () => move(1),
    }),
    registerAction({
      id: "tab.previous",
      scope: "global",
      title: "Previous tab",
      shortcut: ["Mod+Shift+Tab", "Mod+PageUp"],
      run: () => move(-1),
    }),
    registerAction({
      id: "tab.reopen",
      scope: "global",
      title: "Reopen closed tab",
      shortcut: "Mod+Shift+T",
      run: () => s().reopenTab(),
    }),
    registerAction({
      id: "app.palette",
      scope: "global",
      title: "Command palette",
      shortcut: ["Mod+K", "Mod+Shift+P"],
      run: () => s().openPalette(),
    }),
    registerAction({
      id: "app.settings",
      scope: "global",
      title: "Settings",
      shortcut: "Mod+,",
      run: () => workbench().openSettings(),
    }),
  ];
  return () => unregister.forEach((u) => u());
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
