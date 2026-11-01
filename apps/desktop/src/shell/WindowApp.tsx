/** A window beside the main one: a tab moved there (`tab-<n>`), or the quick query of the tray
 *  icon (`quick`). It shows that one tab, with sessions of its own; what it cannot do itself
 *  (open another tab, the settings) goes to the main window. Closing it hands the tab back to
 *  the main window, which can reopen it like a tab closed there. */
import { type ComponentType, Fragment, type ReactNode, useEffect, useRef, useState } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Capabilities, type CompletionCatalog, type SavedConnection, toCommandError } from "../api";
import { asksPassword, isProduction } from "../connections";
import { withDriver } from "../drivers/DriverPopup";
import { PasswordPrompt, persistTab } from "../App";
import { createTab, tabKind, type TabModel, type TabProps } from "../tabs/registry";
import { provideWorkbench } from "../workbench";
import { WINDOW_EVENTS, WINDOW_TAB_KEY } from "./windows";
import { LockScreen } from "../guards/AppLock";
import { track } from "./activity";
import { t } from "../i18n";
import { withInitApproval } from "../ui/initApproval";
import { EmptyState } from "../ui/State";
import { Toaster } from "../ui/Toaster";

/** The connection the quick query used last. */
const QUICK_KEY = "platypus.quick.connection";

export function WindowApp({ label }: { label: string }) {
  const quick = label === "quick";
  const [connections, setConnections] = useState<SavedConnection[] | null>(null);
  const [tab, setTab] = useState<TabModel | null>(null);
  const [catalog, setCatalog] = useState<CompletionCatalog | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [dialogs, setDialogs] = useState<{ id: number; node: ReactNode }[]>([]);
  const [asking, setAsking] = useState<{ connection: SavedConnection; error: string | null; resolve: (p: string | null) => void } | null>(null);
  const session = useRef<Promise<number | null> | null>(null);
  const side = useRef<Promise<number | null> | null>(null);
  const passwords = useRef(new Map<string, string>());
  const capabilities = useRef(new Map<number, Capabilities>());
  const nextDialog = useRef(1);
  const latest = useRef(tab);
  latest.current = tab;
  const [locked, setLocked] = useState(false);
  /** Closing was asked about, with a transaction open or changes not saved. */
  const [closing, setClosing] = useState(false);
  const closeConfirmed = useRef(false);
  const storeKey = WINDOW_TAB_KEY + label;
  const forget = () => {
    try {
      localStorage.removeItem(storeKey);
    } catch {
      // Nothing was kept.
    }
  };

  /** Ends the window's sessions. */
  function endSessions() {
    for (const opening of [session.current, side.current]) {
      void opening?.then((id) => {
        if (id !== null) void api.closeSession(id);
      });
    }
  }

  useEffect(() => {
    void (async () => {
      const all = await api.listConnections().catch(() => [] as SavedConnection[]);
      setConnections(all);
      if (quick) {
        let last: string | null = null;
        try {
          last = localStorage.getItem(QUICK_KEY);
        } catch {
          // No storage: the first connection.
        }
        const connection = all.find((c) => c.id === last) ?? all[0];
        if (connection) setTab(createTab("query", connection.id));
        return;
      }
      const text = await api.takeWindowTab(label).catch(() => null);
      const saved = text ? (JSON.parse(text) as Record<string, unknown> & { kind?: string; connectionId: string }) : null;
      const kind = saved && tabKind(saved.kind ?? "query");
      if (saved && kind) setTab(createTab(kind.kind, saved.connectionId, kind.restore(saved)));
    })();
  }, []);

  // Closed: the tab goes back to the main window, which keeps it to reopen; its sessions end.
  // With a transaction open or changes not saved, asked first, as a tab closing there is.
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      const current = latest.current;
      if (!closeConfirmed.current && (current?.txOpen || current?.unsaved)) {
        event.preventDefault();
        setClosing(true);
        return;
      }
      const saved = current && persistTab(current);
      if (saved && !quick) {
        const handed = await emitTo("main", WINDOW_EVENTS.closed, saved).then(
          () => true,
          () => false,
        );
        if (handed) forget();
      }
      endSessions();
    });
    return () => void unlisten.then((f) => f());
  }, []);

  // Kept while the window is open, for a run that ends without closing it.
  useEffect(() => {
    if (quick || !tab) return;
    const saved = persistTab(tab);
    try {
      if (saved) localStorage.setItem(storeKey, JSON.stringify(saved));
    } catch {
      // Not kept: the main window takes it back when this one closes.
    }
  }, [tab]);

  // The app's lock reaches this window too: production sessions end, passwords typed are
  // forgotten, and the window waits behind the master password. The user at work here keeps
  // the main window's idle lock waiting.
  const latestConnection = useRef<SavedConnection | undefined>(undefined);
  useEffect(() => {
    const stops = [
      listen(WINDOW_EVENTS.locked, () => {
        passwords.current.clear();
        if (isProduction(latestConnection.current)) {
          endSessions();
          session.current = side.current = null;
          setCatalog(null);
        }
        setLocked(true);
      }),
      listen(WINDOW_EVENTS.unlocked, () => setLocked(false)),
    ];
    let last = 0;
    const active = () => {
      if (Date.now() - last < 30_000) return;
      last = Date.now();
      void emitTo("main", WINDOW_EVENTS.activity).catch(() => {});
    };
    const kinds = ["keydown", "mousedown", "wheel"] as const;
    for (const k of kinds) window.addEventListener(k, active, { capture: true, passive: true });
    return () => {
      stops.forEach((s) => void s.then((stop) => stop()));
      for (const k of kinds) window.removeEventListener(k, active, { capture: true });
    };
  }, []);

  const connection = tab ? connections?.find((c) => c.id === tab.connectionId) : undefined;
  latestConnection.current = connection;
  useEffect(() => {
    if (connection) void getCurrentWindow().setTitle(quick ? `${t("Quick query")} · ${connection.name}` : `${tabKind(tab!.kind)?.label(tab as never) ?? ""} · ${connection.name}`);
  }, [connection?.id, tab?.title]);

  function askPassword(c: SavedConnection, error: string | null = null) {
    return new Promise<string | null>((resolve) => setAsking({ connection: c, error, resolve }));
  }

  /** Opens a session on `c`, asking for the password when it is not saved; `null` if not. */
  async function connect(c: SavedConnection, database: string | null, sideSession: boolean): Promise<number | null> {
    let password = passwords.current.get(c.id);
    if (password === undefined && asksPassword(c)) {
      const entered = await askPassword(c);
      if (entered === null) return null;
      password = entered;
    }
    for (;;) {
      try {
        const typed = password;
        const opened = await track(t("Connect to {name}", { name: c.name }), withDriver(() => withInitApproval(c, (init) => api.openSession(c.id, typed, database, sideSession, init))));
        if (password !== undefined) passwords.current.set(c.id, password);
        capabilities.current.set(opened.session_id, opened.capabilities);
        return opened.session_id;
      } catch (e) {
        const error = toCommandError(e);
        if (error.kind !== "auth") {
          setToast(`${c.name}: ${error.message}`);
          return null;
        }
        const entered = await askPassword(c, error.message);
        if (entered === null) return null;
        password = entered;
      }
    }
  }

  const sideSession = (c: SavedConnection, database?: string | null) => (side.current ??= connect(c, database ?? null, true));

  provideWorkbench({
    connections: () => connections ?? [],
    setConnections,
    projects: () => [],
    setProjects: () => {},
    projectFolders: () => [],
    setWorkspace: (w) => setConnections(w.connections),
    activeProject: () => null,
    switchProject: () => {},
    newConnection: () => {},
    exportConnections: () => {},
    newConnectionFrom: () => {},
    duplicateConnection: async () => {},
    editProject: () => {},
    deleteProject: async () => {},
    // Other tabs open in the main window.
    openTab: (kind, connectionId, init) => void emitTo("main", WINDOW_EVENTS.openTab, { kind, connectionId, init }),
    openTemporaryTab: () => setToast(t("Open it from the main window.")),
    focusTab: () => false,
    sideSession,
    openSession: (c, s = false) => connect(c, null, s),
    closeSession: (id) => void api.closeSession(id),
    capabilities: (id) => capabilities.current.get(id),
    showError: setToast,
    notify: setToast,
    openSettings: () => setToast(t("The settings are in the main window.")),
    openDialog: (render) => {
      const id = nextDialog.current++;
      const close = () => setDialogs((all) => all.filter((d) => d.id !== id));
      setDialogs((all) => [...all, { id, node: render(close) }]);
    },
  });

  /** The quick query on another connection: its text stays, its session is a new one. */
  function switchConnection(id: string) {
    if (!tab) return;
    endSessions();
    session.current = side.current = null;
    setCatalog(null);
    const sql = "sql" in tab ? (tab.sql as string) : "";
    setTab(createTab("query", id, { sql }));
    try {
      localStorage.setItem(QUICK_KEY, id);
    } catch {
      // Not kept: the first connection next time.
    }
  }

  async function moveToMain() {
    // Its session stays here: what it holds would be lost.
    if (tab?.txOpen || tab?.unsaved) return setToast(t("Commit or roll back, and save the grid's changes, before moving the tab: its session stays here."));
    const saved = tab && persistTab(tab);
    if (!saved) return;
    await emitTo("main", WINDOW_EVENTS.attach, saved);
    forget();
    latest.current = null;
    endSessions();
    await getCurrentWindow().destroy();
  }

  const Component = tab && (tabKind(tab.kind)?.component as ComponentType<TabProps<TabModel>> | undefined);
  return (
    <div className="window-app">
      <div className="window-bar">
        {quick ? (
          <label className="window-connection">
            <span className="muted small">{t("Connection")}</span>
            <select value={tab?.connectionId ?? ""} onChange={(e) => switchConnection(e.target.value)}>
              {(connections ?? []).map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <strong className="window-title">{connection?.name}</strong>
        )}
        <span className="spacer" />
        {!quick && tab && (
          <button type="button" onClick={() => void moveToMain()}>
            {t("Move to the main window")}
          </button>
        )}
      </div>
      {toast && (
        <div className="notice error toast" role="alert" onClick={() => setToast(null)}>
          {toast}
        </div>
      )}
      <div className="panes">
        {connections && connections.length === 0 && <EmptyState className="window-empty" title={t("No connection is saved yet.")} />}
        {tab && connection && Component && (
          <div className="pane">
            <Component
              key={tab.key}
              tab={tab}
              connection={connection}
              active
              ensureSession={() => (session.current ??= connect(connection, tab.database ?? null, false))}
              currentSession={() => session.current}
              onChange={(patch) => setTab((now) => (now ? ({ ...now, ...patch } as TabModel) : now))}
              sideSession={() => sideSession(connection, tab.database)}
              catalog={catalog}
              onNeedCatalog={(force) => {
                if (catalog && !force) return;
                void sideSession(connection, tab.database).then(async (id) => {
                  if (id !== null) setCatalog(await api.completionCatalog(id).catch(() => null));
                });
              }}
              onError={setToast}
            />
          </div>
        )}
      </div>
      {dialogs.map((d) => (
        <Fragment key={d.id}>{d.node}</Fragment>
      ))}
      {closing && (
        <div className="modal-backdrop">
          <div className="modal small" onKeyDown={(e) => e.key === "Escape" && setClosing(false)}>
            <h2>{t("Close this window?")}</h2>
            <p className="muted small">{t("Its tab has an open transaction or changes not saved: closing rolls them back.")}</p>
            <div className="actions">
              <span className="spacer" />
              <button type="button" onClick={() => setClosing(false)} autoFocus>
                {t("Cancel")}
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => {
                  closeConfirmed.current = true;
                  void getCurrentWindow().close();
                }}
              >
                {t("Close")}
              </button>
            </div>
          </div>
        </div>
      )}
      {locked && (
        <LockScreen
          onUnlocked={() => {
            setLocked(false);
            void emitTo("main", WINDOW_EVENTS.unlocked).catch(() => {});
          }}
        />
      )}
      {asking && (
        <PasswordPrompt
          connection={asking.connection}
          error={asking.error}
          onDone={(password) => {
            asking.resolve(password);
            setAsking(null);
          }}
        />
      )}
      <Toaster />
    </div>
  );
}
