import { type ComponentType, Fragment, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { dispatchKey, registerAction } from "./actions";
import { api, type Capabilities, type CompletionCatalog, type Project, toCommandError, type SavedConnection } from "./api";
import { onNavigatorRefresh } from "./navigatorEvents";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { ImportDialog } from "./components/ImportDialog";
import { UnlockDialog } from "./components/MasterPassword";
import { asksPassword, connectionColor } from "./connections";
import { Navigator } from "./components/Navigator";
import { SettingsDialog } from "./components/SettingsDialog";
import { UpdateBanner } from "./components/UpdateBanner";
import { CommandPalette } from "./shell/CommandPalette";
import "./tabs";
import "./objects/actions";
import { type TabKindName, type TabModel, type TabProps, createTab, tabKind } from "./tabs/registry";
import { dbKey, provideWorkbench, workbench } from "./workbench";
import { Icon, type IconName } from "./ui/Icon";
import { type PersistedTab, loadWorkspace, saveWorkspace } from "./workspace";
import { connectionsOf, loadActiveProject, projectKey, saveActiveProject } from "./projects/state";
import { ProjectRail } from "./projects/ProjectRail";
import { ProjectDialog } from "./projects/ProjectDialog";
import { AddConnectionsDialog } from "./projects/AddConnectionsDialog";
import { registerAddToProject } from "./projects/actions";
import { ScriptsPanel } from "./scripts/ScriptsPanel";
import { ContextMenu, type MenuState } from "./components/ContextMenu";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Dialog =
  | { kind: "connection"; connection: SavedConnection; existing: boolean; memberOf: string[] }
  | { kind: "add-connections" }
  | { kind: "import" }
  | { kind: "project"; project: Project | null }
  | null;

interface PasswordRequest {
  connection: SavedConnection;
  resolve: (password: string | null) => void;
}

export function App() {
  const [connections, setConnections] = useState<SavedConnection[] | null>(null);
  /** `null` until loaded. */
  const [projects, setProjects] = useState<Project[] | null>(null);
  /** The project shown; `null` is All connections. */
  const [activeProject, setActiveProject] = useState<string | null>(null);
  /** The active tab of each project while another one is shown, by `projectKey`. */
  const activeByProject = useRef(new Map<string, number>());
  const [railMenu, setRailMenu] = useState<MenuState | null>(null);
  const [storeError, setStoreError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest | null>(null);
  const [unlockRequest, setUnlockRequest] = useState<{ resolve: (unlocked: boolean) => void } | null>(null);
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
  /** Set by the restore, a render after it: the first save then sees the restored tabs. */
  const [workspaceReady, setWorkspaceReady] = useState(false);

  useEffect(() => {
    api.listConnections().then(setConnections, (e: unknown) => {
      setConnections([]);
      setStoreError(toCommandError(e).message);
    });
    // Saved passwords behind a master password: asked for once at start.
    api.vaultStatus().then(
      (s) => s.enabled && !s.unlocked && void askUnlock(),
      () => {},
    );
    api.listProjects().then(
      (all) => {
        setProjects(all);
        setActiveProject(loadActiveProject(all));
      },
      () => setProjects([]),
    );
    api.appInfo().then((i) => {
      setVersion(i.version);
      // The end-to-end tests drive what has no button yet through these.
      if (i.devtools) {
        (window as unknown as { __platypus: object }).__platypus = {
          switchProject: (id: string | null) => shell.current.switchProject(id),
        };
      }
    }, console.error);
  }, []);

  // Restore the previous workspace once connections and projects are known.
  useEffect(() => {
    if (!connections || !projects || restored.current) return;
    restored.current = true;
    setWorkspaceReady(true);
    const saved = loadWorkspace();
    if (!saved) return;
    const known = new Set(connections.map((c) => c.id));
    const existing = new Set(projects.map((p) => p.id));
    // By saved index, so the active tabs (saved as indices) find theirs.
    const byIndex = saved.tabs.map((t) => {
      // Tabs saved before tab kinds existed are SQL editors.
      const kind = tabKind(t.kind ?? "query");
      if (!kind || !known.has(t.connectionId)) return null;
      const tab = createTab(kind.kind, t.connectionId, kind.restore(t));
      // A deleted project's tabs go to All connections.
      tab.project = t.project && existing.has(t.project) ? t.project : null;
      return tab;
    });
    for (const [key, index] of Object.entries(saved.active)) {
      const tab = byIndex[index];
      if (tab) activeByProject.current.set(key, tab.key);
    }
    const restoredTabs = byIndex.filter((t) => t !== null);
    setTabs(restoredTabs);
    setActiveTab(shownTab(restoredTabs, activeProject));
  }, [connections, projects]);

  useEffect(() => {
    if (!workspaceReady) return;
    rememberActiveTab();
    const kept = tabs.flatMap((tab) => {
      const saved = persistTab(tab);
      return saved ? [{ key: tab.key, saved }] : [];
    });
    const active: Record<string, number> = {};
    for (const [key, tabKey] of activeByProject.current) {
      const index = kept.findIndex((k) => k.key === tabKey);
      if (index >= 0) active[key] = index;
    }
    saveWorkspace({ tabs: kept.map((k) => k.saved), active });
  }, [workspaceReady, tabs, activeTab, activeProject]);

  const project = projects?.find((p) => p.id === activeProject) ?? null;
  const projectConnections = useMemo(() => connectionsOf(project, connections ?? []), [project, connections]);
  // The window says which project is shown.
  useEffect(() => {
    const title = project ? `${project.name} — PlatypusDB` : "PlatypusDB";
    document.title = title;
    getCurrentWindow()
      .setTitle(title)
      .catch((e: unknown) => console.warn("window title", e));
  }, [project?.name]);

  /** The tabs of the active project, the ones the tab bar shows. */
  const visibleTabs = tabs.filter((t) => inProject(t, activeProject));

  /** Projects with an open transaction in one of their tabs, for the rail's warning dot. */
  const txProjects = new Set(tabs.filter((t) => t.txOpen).map((t) => projectKey(t.project)));

  /** Deletes a project once confirmed: its tabs close (those with changes ask first, and a
   *  refusal keeps the project), its connections stay. */
  async function deleteProject(p: Project) {
    if (!window.confirm(`Delete the project "${p.name}"? Its connections stay, in All connections and other projects.`)) {
      return;
    }
    const own = tabs.filter((t) => t.project === p.id);
    if (own.some((t) => t.unsaved || t.txOpen) && !window.confirm("Some of its tabs have changes not saved or committed. Close them and discard the changes?")) {
      return;
    }
    try {
      setProjects(await api.deleteProject(p.id));
    } catch (e) {
      return setToast(toCommandError(e).message);
    }
    for (const t of own) closeTab(t, true);
    activeByProject.current.delete(p.id);
    if (activeProject === p.id) showProject(null);
  }

  /** Keeps the active tab of the project shown, to show it again on coming back. */
  function rememberActiveTab() {
    activeByProject.current.set(projectKey(activeProject), activeTab ?? -1);
  }

  /** The tab to show in a project: the one active there last, else its last one. */
  function shownTab(all: TabModel[], id: string | null): number | null {
    const own = all.filter((t) => inProject(t, id));
    const remembered = activeByProject.current.get(projectKey(id));
    return own.find((t) => t.key === remembered)?.key ?? own.at(-1)?.key ?? null;
  }

  /** Makes a project the active one, kept for the next start, with its tab. */
  function showProject(id: string | null) {
    setActiveProject(id);
    saveActiveProject(id);
    setActiveTab(shownTab(tabs, id));
  }

  /** Shows another project with the tab that was active there. Nothing closes. */
  function switchProject(id: string | null) {
    if (id === activeProject || (id !== null && !projects?.some((p) => p.id === id))) return;
    rememberActiveTab();
    showProject(id);
  }

  const connectionById = (id: string) => connections?.find((c) => c.id === id);

  /** A new connection goes into the active project. */
  async function newConnection() {
    setDialog({
      kind: "connection",
      connection: await api.defaultConnection("postgres"),
      existing: false,
      memberOf: activeProject ? [activeProject] : [],
    });
  }

  /** After a command that saved connections: the projects they joined or left changed too. */
  async function connectionsSaved(all: SavedConnection[]) {
    setConnections(all);
    setDialog(null);
    try {
      setProjects(await api.listProjects());
    } catch (e) {
      setToast(toCommandError(e).message);
    }
  }

  async function deleteConnection(c: SavedConnection) {
    if (!window.confirm(`Delete the connection "${c.name}"?`)) return;
    try {
      setConnections(await api.deleteConnection(c.id));
    } catch (e) {
      return setToast(toCommandError(e).message);
    }
    for (const t of tabs.filter((t) => t.connectionId === c.id)) closeTab(t, true);
    disconnectNavigator(c);
    try {
      // The store took the connection out of every project.
      setProjects(await api.listProjects());
    } catch (e) {
      setToast(toCommandError(e).message);
    }
  }

  /** Opens a tab in the active project, unless it says which one it belongs to. */
  function addTab(tab: TabModel) {
    const placed = tab.project === undefined ? { ...tab, project: activeProject } : tab;
    setTabs((all) => [...all, placed]);
    setActiveTab(placed.key);
  }

  /** Asks for the master password; true once the saved passwords are unlocked. */
  function askUnlock() {
    return new Promise<boolean>((resolve) => setUnlockRequest({ resolve }));
  }

  function askPassword(connection: SavedConnection) {
    return new Promise<string | null>((resolve) => setPasswordRequest({ connection, resolve }));
  }

  /** Opens a session, asking for the password when it is not saved; `null` on failure.
   *  `database` opens another database of the server than the saved one. */
  async function connect(connection: SavedConnection, database?: string | null): Promise<number | null> {
    let password: string | undefined;
    if (asksPassword(connection)) {
      password = passwords.current.get(connection.id);
      if (password === undefined) {
        const entered = await askPassword(connection);
        if (entered === null) return null;
        password = entered;
      }
    }
    try {
      let session;
      try {
        session = await api.openSession(connection.id, password, database);
      } catch (e) {
        // Saved passwords behind a master password not entered yet: ask, then try again.
        if (toCommandError(e).kind !== "locked" || !(await askUnlock())) throw e;
        session = await api.openSession(connection.id, password, database);
      }
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
    const saved = persistTab(tab);
    if (saved) {
      closedTabs.current.push(saved);
      if (closedTabs.current.length > CLOSED_TABS) closedTabs.current.shift();
    }
    const session = sessions.current.get(tab.key);
    sessions.current.delete(tab.key);
    void session?.then((id) => {
      if (id !== null) void api.closeSession(id);
    });
    setTabs((all) => {
      const rest = all.filter((t) => t.key !== tab.key);
      // The next tab shown is one of the same project.
      const same = rest.filter((t) => inProject(t, tab.project ?? null));
      setActiveTab((active) => (active === tab.key ? (same.at(-1)?.key ?? null) : active));
      return rest;
    });
  }

  // What the shell's actions and the workbench need, refreshed every render so they
  // always see the latest state while being registered once.
  /** Reopens the tab closed last in the active project. */
  const reopenTab = () => {
    const closed = closedTabs.current;
    let index = closed.length - 1;
    while (index >= 0 && !inProject(closed[index]!, activeProject)) index--;
    const saved = index >= 0 ? closed.splice(index, 1)[0] : undefined;
    const kind = saved && tabKind((saved.kind ?? "query") as TabKindName);
    if (!saved || !kind || !connectionById(saved.connectionId)) return;
    addTab(createTab(kind.kind, saved.connectionId, kind.restore(saved)));
  };
  const openPalette = () => setPaletteOpen(true);
  // `tabs` here are the active project's: the tab actions work on what the bar shows.
  const shellState = {
    tabs: visibleTabs,
    activeTab,
    connections,
    projects: projects ?? [],
    activeProject,
    addTab,
    closeTab,
    setActiveTab,
    navigatorSession,
    reopenTab,
    openPalette,
    switchProject,
    deleteProject,
  };
  const shell = useRef(shellState);
  shell.current = shellState;

  // The active tab stays in view when tabs overflow the bar.
  useEffect(() => {
    tabBar.current?.querySelector(".tab.active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [activeTab, tabs.length]);

  provideWorkbench({
    connections: () => shell.current.connections ?? [],
    setConnections,
    projects: () => shell.current.projects,
    setProjects,
    activeProject: () => shell.current.projects.find((p) => p.id === shell.current.activeProject) ?? null,
    switchProject: (id) => shell.current.switchProject(id),
    editProject: (project) => setDialog({ kind: "project", project }),
    deleteProject: (project) => shell.current.deleteProject(project),
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
  // "Add to project" lists the projects as they are.
  useEffect(() => registerAddToProject(projects ?? []), [projects]);

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
      <ProjectRail
        projects={projects ?? []}
        active={activeProject}
        txProjects={txProjects}
        onSwitch={switchProject}
        onNew={() => setDialog({ kind: "project", project: null })}
        onMenu={(p, e) => setRailMenu({ x: e.clientX, y: e.clientY, target: { scope: "project", arg: p } })}
      />
      <div className="sidebar-column">
        <Navigator
          connections={projectConnections}
          project={project}
          onAddExisting={() => setDialog({ kind: "add-connections" })}
          error={storeError}
          onNew={() => void newConnection()}
          onImport={() => setDialog({ kind: "import" })}
          onEdit={(c) =>
            setDialog({
              kind: "connection",
              connection: c,
              existing: true,
              memberOf: (projects ?? []).filter((p) => p.connections.includes(c.id)).map((p) => p.id),
            })
          }
          onDelete={(c) => void deleteConnection(c)}
          onOpenTab={(c, init) => addTab(createTab("query", c.id, init))}
          session={navigatorSession}
          capabilities={(sessionId) => capabilities.current.get(sessionId)}
          disconnect={disconnectNavigator}
          onRefresh={(c, database) => requestCatalog(c, true, database)}
          onError={setToast}
        />
        <ScriptsPanel project={project} connections={projectConnections} onError={setToast} />
      </div>
      <main className="workspace">
        <UpdateBanner />
        {visibleTabs.length > 0 && (
          <div
            className="tabs"
            ref={tabBar}
            // The wheel scrolls the tabs sideways: the bar has nothing to scroll up or down.
            onWheel={(e) => {
              if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
            }}
          >
            {visibleTabs.map((t) => (
              <div
                key={t.key}
                className={`tab${t.key === activeTab ? " active" : ""}`}
                style={{ borderTopColor: connectionColor(connectionById(t.connectionId)) ?? "transparent" }}
                onClick={() => setActiveTab(t.key)}
                onAuxClick={(e) => e.button === 1 && closeTab(t)}
              >
                <span className="tab-icon">
                  <Icon name={tabIcon(t)} size={13} />
                </span>
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
                currentSession={() => sessions.current.get(t.key) ?? null}
                onChange={(patch) => patchTab(t.key, patch)}
                sideSession={() => navigatorSession(connection, t.database)}
                catalog={catalogs[dbKey(connection.id, t.database)] ?? null}
                onNeedCatalog={(force) => requestCatalog(connection, force, t.database)}
                onError={setToast}
              />
            )
          );
        })}
        {visibleTabs.length === 0 && (
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
          projects={projects ?? []}
          memberOf={dialog.memberOf}
          onClose={() => setDialog(null)}
          onSaved={(all) => void connectionsSaved(all)}
        />
      )}
      {dialog?.kind === "add-connections" && project && (
        <AddConnectionsDialog
          project={project}
          connections={connections ?? []}
          onClose={() => setDialog(null)}
          onSaved={(all) => {
            setProjects(all);
            setDialog(null);
          }}
        />
      )}
      {dialog?.kind === "project" && (
        <ProjectDialog
          project={dialog.project}
          onClose={() => setDialog(null)}
          onSaved={(all, saved) => {
            const created = dialog.project === null;
            setProjects(all);
            setDialog(null);
            // A new project is where the user goes next.
            if (created) {
              rememberActiveTab();
              showProject(saved.id);
            }
          }}
        />
      )}
      {railMenu && <ContextMenu menu={railMenu} onClose={() => setRailMenu(null)} onError={(e) => setToast(toCommandError(e).message)} />}
      {dialog?.kind === "import" && (
        <ImportDialog
          existing={connections ?? []}
          project={activeProject}
          onClose={() => setDialog(null)}
          onImported={(all) => void connectionsSaved(all)}
        />
      )}
      {settingsOpen && <SettingsDialog onClose={() => setSettingsOpen(false)} />}
      {paletteOpen && (
        <CommandPalette
          connections={connectionsOf(project, connections ?? [])}
          catalogs={catalogs}
          onClose={() => setPaletteOpen(false)}
        />
      )}
      {dialogs.map((d) => (
        <Fragment key={d.id}>{d.node}</Fragment>
      ))}
      {unlockRequest && (
        <UnlockDialog
          onDone={(unlocked) => {
            unlockRequest.resolve(unlocked);
            setUnlockRequest(null);
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


/** A tab's icon: what it shows. */
function tabIcon(t: TabModel): IconName {
  if (t.kind === "object") return "object";
  if (t.kind === "builder") return "builder";
  return "tableRef" in t && t.tableRef ? "table" : "sql";
}

/** A tab as the workspace saves it, or `null` for a kind that is not kept. */
function persistTab(tab: TabModel): PersistedTab | null {
  const state = tabKind(tab.kind)?.persist(tab);
  return state ? { ...state, kind: tab.kind, connectionId: tab.connectionId, project: tab.project ?? null } : null;
}

/** The tab belongs to the project (`null` for All connections); tabs saved before projects
 *  have none. */
function inProject(tab: { project?: string | null }, id: string | null): boolean {
  return (tab.project ?? null) === id;
}

interface ShellApi {
  /** The active project's tabs. */
  tabs: TabModel[];
  activeTab: number | null;
  connections: SavedConnection[] | null;
  projects: Project[];
  activeProject: string | null;
  switchProject: (id: string | null) => void;
  deleteProject: (project: Project) => Promise<void>;
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
