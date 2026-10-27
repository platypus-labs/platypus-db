import { type ComponentType, type CSSProperties, Fragment, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { dispatchKey, registerAction } from "./actions";
import {
  api,
  type Capabilities,
  type CompletionCatalog,
  isDataFile,
  type Project,
  type ProjectFolder,
  type Workspace,
  toCommandError,
  type SavedConnection,
} from "./api";
import { onNavigatorRefresh, requestNavigatorRefresh } from "./navigatorEvents";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { ImportDialog } from "./components/ImportDialog";
import { UnlockDialog } from "./components/MasterPassword";
import { asksPassword, connectionColor } from "./connections";
import { sessionSettingsChanged } from "./connectionEdit";
import { Navigator } from "./components/Navigator";
import { SettingsDialog } from "./components/SettingsDialog";
import { UpdateBanner } from "./components/UpdateBanner";
import { CommandPalette } from "./shell/CommandPalette";
import "./tabs";
import "./objects/actions";
import "./transfer/actions";
import "./querylog/actions";
import { setAnswer } from "./testAnswers";
import { type TabKindName, type TabModel, type TabProps, createTab, tabKind } from "./tabs/registry";
import { dbKey, provideWorkbench, workbench } from "./workbench";
import type { IconName } from "./ui/Icon";
import { ask, isAsking } from "./ui/ask";
import { type PersistedTab, flushWorkspace, loadWorkspace, saveWorkspace } from "./workspace";
import { connectionsOf, loadActiveProject, projectKey, saveActiveProject } from "./projects/state";
import { forgetCatalog } from "./catalog/catalog";
import { ProjectRail } from "./projects/ProjectRail";
import { ProjectDialog } from "./projects/ProjectDialog";
import { AddConnectionsDialog } from "./projects/AddConnectionsDialog";
import { registerAddToProject } from "./projects/actions";
import { registerMoveToFolder } from "./projects/folders";
import { withDriver } from "./drivers/DriverPopup";
import { ScriptsPanel } from "./scripts/ScriptsPanel";
import { ContextMenu, type MenuState } from "./components/ContextMenu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { TopBar } from "./shell/TopBar";
import { Welcome } from "./shell/Welcome";
import { TabBar, barOrder, groupTabsSetting } from "./shell/TabBar";
import { SidebarResizer, useSidebarWidth } from "./shell/SidebarResizer";
import { useSetting } from "./settings";
import { useSidebarCollapsed } from "./shell/sidebar";
import { language, languageSetting, t, plural } from "./i18n";
import { stepZoom } from "./theme";
import { logError, track } from "./shell/activity";
import { openActivity } from "./shell/ActivityDialog";

declare module "./actions" {
  interface ActionScopes {
    /** A tab of the bar, by its context menu. */
    tab: { tab: TabModel };
  }
}

type Dialog =
  | {
      kind: "connection";
      connection: SavedConnection;
      existing: boolean;
      memberOf: string[];
      driverChosen?: boolean;
      /** The folder of the active project the new connection goes into. */
      folder?: string | null;
    }
  | { kind: "add-connections" }
  | { kind: "import" }
  | { kind: "project"; project: Project | null }
  | null;

interface PasswordRequest {
  connection: SavedConnection;
  /** The server's answer to the password tried before. */
  error: string | null;
  resolve: (password: string | null) => void;
}

export function App() {
  const [connections, setConnections] = useState<SavedConnection[] | null>(null);
  /** `null` until loaded. */
  const [projects, setProjects] = useState<Project[] | null>(null);
  const [projectFolders, setProjectFolders] = useState<ProjectFolder[]>([]);
  /** The project shown; `null` is All connections. */
  const [activeProject, setActiveProject] = useState<string | null>(null);
  /** The active tab of each project while another one is shown, by `projectKey`. */
  const activeByProject = useRef(new Map<string, number>());
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [storeError, setStoreError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest | null>(null);
  const [unlockRequest, setUnlockRequest] = useState<{ resolve: (unlocked: boolean) => void } | null>(null);
  const [tabs, setTabs] = useState<TabModel[]>([]);
  const [activeTab, setActiveTab] = useState<number | null>(null);
  /** The two tabs shown side by side, left first; the active tab is one of them. */
  const [split, setSplit] = useState<[number, number] | null>(null);
  /** The split's pane that had the focus last: the next tab activated replaces it. */
  const focusedPane = useRef<number | null>(null);
  /** Connections whose group of tabs is folded. */
  const [folded, setFolded] = useState<ReadonlySet<string>>(new Set());
  const grouped = useSetting(groupTabsSetting);
  // A new language shows at once: everything below renders again.
  useSetting(languageSetting);
  const [toast, showToast] = useState<string | null>(null);
  const [sidebarWidth, setSidebarWidth] = useSidebarWidth();
  /** Shows an error, and keeps it in the Activity log. */
  const setToast = (message: string | null) => {
    showToast(message);
    if (message) logError(message);
  };
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
  /** Connections that are not saved (the DuckDB of "Query with SQL"), by id, with what to do
   *  once their last tab is closed. */
  const temporary = useRef(new Map<string, { connection: SavedConnection; onClosed: () => void }>());
  /** Navigator sessions by `dbKey`, separate from the editors' so browsing never waits
   *  behind a running query. */
  const navSessions = useRef(new Map<string, Promise<number | null>>());
  /** The connection of every open session (editors and navigator), to mark it as active. */
  const [openSessions, setOpenSessions] = useState<ReadonlyMap<number, string>>(new Map());
  const activeConnections = useMemo(() => new Set(openSessions.values()), [openSessions]);
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
    api.listProjectFolders().then(setProjectFolders, () => setProjectFolders([]));
    api.appInfo().then((i) => {
      setVersion(i.version);
      // The end-to-end tests drive what has no button yet through these.
      if (i.devtools) {
        (window as unknown as { __platypus: object }).__platypus = {
          switchProject: (id: string | null) => shell.current.switchProject(id),
          // What a file dropped on the window does; CDP cannot drop files.
          openDroppedFile: (path: string) => openDroppedFile(path),
          // The folder picker and the clipboard.
          answer: setAnswer,
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
      tab.pinned = t.pinned === true;
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
      // A temporary connection is gone at the next start: its tabs are not kept.
      const saved = temporary.current.has(tab.connectionId) ? null : persistTab(tab);
      return saved ? [{ key: tab.key, saved }] : [];
    });
    const active: Record<string, number> = {};
    for (const [key, tabKey] of activeByProject.current) {
      const index = kept.findIndex((k) => k.key === tabKey);
      if (index >= 0) active[key] = index;
    }
    saveWorkspace({ tabs: kept.map((k) => k.saved), active });
  }, [workspaceReady, tabs, activeTab, activeProject]);

  // Closing the window asks first, as closing the tabs does, when some have edits not saved or
  // a transaction open; the workspace is written at once instead of after its debounce.
  const tabsNow = useRef(tabs);
  tabsNow.current = tabs;
  useEffect(() => {
    const off = getCurrentWindow().onCloseRequested(async (event) => {
      flushWorkspace();
      const changed = tabsNow.current.filter((tab) => tab.unsaved || tab.txOpen).length;
      const question = plural(
        changed,
        "One tab has changes not saved or committed. Close the window and discard them?",
        "{n} tabs have changes not saved or committed. Close the window and discard them?",
      );
      if (changed > 0 && !(await ask(question, { confirm: t("Close"), danger: true }))) event.preventDefault();
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);

  // A temporary connection goes with its last tab: its navigator sessions close, then it.
  useEffect(() => {
    for (const [id, temp] of temporary.current) {
      if (tabs.some((t) => t.connectionId === id)) continue;
      temporary.current.delete(id);
      disconnectNavigator(temp.connection);
      temp.onClosed();
    }
  }, [tabs]);

  const project = projects?.find((p) => p.id === activeProject) ?? null;
  const projectConnections = useMemo(() => connectionsOf(project, connections ?? []), [project, connections]);
  // The window says which project is shown.
  useEffect(() => {
    const title = project ? `${project.name} — PlatypusDB` : "PlatypusDB";
    document.documentElement.lang = language();
    document.title = title;
    getCurrentWindow()
      .setTitle(title)
      .catch((e: unknown) => console.warn("window title", e));
  }, [project?.name, language()]);

  /** The tabs of the active project, the ones the tab bar shows, in its order. */
  const visibleTabs = barOrder(
    tabs.filter((t) => inProject(t, activeProject)),
    grouped,
  );

  // The split follows the tabs: a tab activated elsewhere takes the focused pane's place, and
  // the split ends when one of its tabs closes or another project is shown.
  useEffect(() => {
    if (!split) return;
    const shown = new Set(visibleTabs.map((t) => t.key));
    if (activeTab === null || !split.every((k) => shown.has(k))) return setSplit(null);
    if (split.includes(activeTab)) {
      focusedPane.current = activeTab;
      return;
    }
    const next: [number, number] = [...split];
    next[Math.max(0, split.indexOf(focusedPane.current ?? -1))] = activeTab;
    focusedPane.current = activeTab;
    setSplit(next);
  });

  /** Projects with an open transaction in one of their tabs, for the rail's warning dot. */
  const txProjects = new Set(tabs.filter((t) => t.txOpen).map((t) => projectKey(t.project)));

  /** Deletes a project once confirmed: its tabs close (those with changes ask first, and a
   *  refusal keeps the project), its connections stay. */
  async function deleteProject(p: Project) {
    if (!(await ask(t('Delete the project "{name}"? Its connections stay, in All connections and other projects.', { name: p.name }), { confirm: t("Delete"), danger: true }))) {
      return;
    }
    const own = tabs.filter((t) => t.project === p.id);
    if (own.some((tab) => tab.unsaved || tab.txOpen) && !(await ask(t("Some of its tabs have changes not saved or committed. Close them and discard the changes?"), { confirm: t("Discard"), danger: true }))) {
      return;
    }
    try {
      setProjects(await api.deleteProject(p.id));
      // Its rail folder lost it, or went away.
      setProjectFolders(await api.listProjectFolders());
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

  const connectionById = (id: string) => connections?.find((c) => c.id === id) ?? temporary.current.get(id)?.connection;
  const connectionName = (id: string) => connectionById(id)?.name ?? "?";

  /** A new connection goes into the active project. */
  async function newConnection(folder: string | null = null) {
    setDialog({
      kind: "connection",
      connection: await api.defaultConnection("postgres"),
      existing: false,
      memberOf: activeProject ? [activeProject] : [],
      folder,
    });
  }

  /** A data file dropped on the window opens as a DuckDB database: a saved connection to it
   *  gets a new script, otherwise the new connection dialog starts filled in. */
  async function openDroppedFile(path: string) {
    // Windows paths ignore case and take either slash.
    const key = (p: string) => (/^[a-z]:/i.test(p) ? p.replace(/\//g, "\\").toLowerCase() : p);
    const saved = shell.current.connections?.find((c) => c.driver === "duckdb" && key(c.database) === key(path));
    if (saved) return workbench().openTab("query", saved.id, {});
    const connection = await api.defaultConnection("duckdb");
    const name = path.split(/[\\/]/).pop() ?? path;
    const active = shell.current.activeProject;
    setDialog({ kind: "connection", connection: { ...connection, name, database: path }, existing: false, memberOf: active ? [active] : [], driverChosen: true });
  }

  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent(({ payload }) => {
      // The import dialog reads the files dropped on it.
      if (shell.current.importOpen) return;
      const path = payload.type === "drop" ? payload.paths.find(isDataFile) : undefined;
      if (path) void openDroppedFile(path).catch((e: unknown) => setToast(toCommandError(e).message));
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);

  function setWorkspace(w: Workspace) {
    setConnections(w.connections);
    setProjects(w.projects);
    setProjectFolders(w.project_folders);
  }

  /** Tabs whose session waits for their transaction to end before following an edit of
   *  their connection. */
  const staleSessions = useRef(new Set<number>());

  /** Closes a tab's session: its next statement opens another one. */
  function dropSession(key: number) {
    const session = sessions.current.get(key);
    sessions.current.delete(key);
    void session?.then((id) => {
      if (id !== null) closeSession(id);
    });
  }

  /** Sessions opened before an edit of `connection` that changed how they connect (another
   *  database, a production type now in manual commit…) are reopened with the saved
   *  settings on their next use. A tab with an open transaction is asked about: now, rolling
   *  it back, or once it ends. */
  async function followEdit(connection: SavedConnection) {
    disconnectNavigator(connection);
    requestNavigatorRefresh({ connectionId: connection.id });
    const own = tabs.filter((tab) => tab.connectionId === connection.id && sessions.current.has(tab.key));
    const busy = own.filter((tab) => tab.txOpen);
    for (const tab of own) if (!tab.txOpen) dropSession(tab.key);
    if (busy.length === 0) return;
    const question = plural(
      busy.length,
      'A tab of "{name}" has an open transaction. Reconnect it now with the new settings, rolling its changes back? Otherwise it reconnects once the transaction ends.',
      '{n} tabs of "{name}" have an open transaction. Reconnect them now with the new settings, rolling their changes back? Otherwise they reconnect once their transaction ends.',
      { name: connection.name },
    );
    const now = await ask(question, { confirm: t("Reconnect now"), danger: true });
    for (const tab of busy) {
      if (now) {
        dropSession(tab.key);
        patchTab(tab.key, { txOpen: false });
      } else {
        staleSessions.current.add(tab.key);
      }
    }
  }

  // A tab waiting for its transaction to end follows the edit once it has.
  useEffect(() => {
    for (const key of staleSessions.current) {
      const tab = tabs.find((t) => t.key === key);
      if (tab?.txOpen) continue;
      staleSessions.current.delete(key);
      if (tab) dropSession(key);
    }
  }, [tabs]);

  /** Saves a copy of the connection, with its passwords, and opens it for editing. */
  async function duplicateConnection(c: SavedConnection) {
    const { connection, workspace } = await api.duplicateConnection(c.id);
    setWorkspace(workspace);
    setDialog({
      kind: "connection",
      connection,
      existing: true,
      memberOf: workspace.projects.filter((p) => p.connections.includes(connection.id)).map((p) => p.id),
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
    if (!(await ask(t('Delete the connection "{name}"?', { name: c.name }), { confirm: t("Delete"), danger: true }))) return;
    try {
      setConnections(await api.deleteConnection(c.id));
    } catch (e) {
      return setToast(toCommandError(e).message);
    }
    for (const t of tabs.filter((t) => t.connectionId === c.id)) closeTab(t, true);
    disconnectNavigator(c);
    forgetCatalog(c.id);
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

  /** The open unlock dialog's answer, shared by every connection waiting for it. */
  const unlocking = useRef<Promise<boolean> | null>(null);

  /** Asks for the master password; true once the saved passwords are unlocked. */
  function askUnlock() {
    unlocking.current ??= new Promise<boolean>((resolve) => setUnlockRequest({ resolve })).finally(() => {
      unlocking.current = null;
    });
    return unlocking.current;
  }

  /** The password asked for each connection, shared by every session waiting for it. */
  const passwordAsks = useRef(new Map<string, Promise<string | null>>());
  /** The password dialog shows one connection at a time: the others wait their turn. */
  const passwordTurn = useRef<Promise<unknown>>(Promise.resolve());

  /** Asks for the connection's password; `error` is why the previous one did not do. */
  function askPassword(connection: SavedConnection, error: string | null = null) {
    const asked = passwordAsks.current.get(connection.id);
    if (asked) return asked;
    const answer = passwordTurn.current
      .then(() => new Promise<string | null>((resolve) => setPasswordRequest({ connection, error, resolve })))
      .finally(() => passwordAsks.current.delete(connection.id));
    passwordTurn.current = answer;
    passwordAsks.current.set(connection.id, answer);
    return answer;
  }

  /** Opens a session, asking for the password when it is not saved; `null` on failure.
   *  `database` opens another database of the server than the saved one. A password the
   *  server refuses (typed, or missing from the keychain) is asked again, with its answer. */
  async function connect(connection: SavedConnection, database?: string | null): Promise<number | null> {
    // Typed this run: asked for, or missing from the keychain.
    let password = passwords.current.get(connection.id);
    let prompted = false;
    if (password === undefined && asksPassword(connection)) {
      const entered = await askPassword(connection);
      if (entered === null) return null;
      password = entered;
    }
    // A database reached through a driver not downloaded yet: offer it, then go on.
    const target = database ? `${connection.name}/${database}` : connection.name;
    for (;;) {
      const typed = password;
      const open = () => track(t("Connect to {name}", { name: target }), withDriver(() => api.openSession(connection.id, typed, database)));
      try {
        let session;
        try {
          session = await open();
        } catch (e) {
          // Saved passwords behind a master password not entered yet: ask, then try again.
          if (toCommandError(e).kind !== "locked" || !(await askUnlock())) throw e;
          session = await open();
        }
        if (typed !== undefined) {
          passwords.current.set(connection.id, typed);
          // "Saved in the keychain" with none there: it is now.
          if (prompted && connection.save_password && !asksPassword(connection)) void api.rememberPassword(connection.id, typed).catch(() => {});
        }
        capabilities.current.set(session.session_id, session.capabilities);
        setOpenSessions((m) => new Map(m).set(session.session_id, connection.id));
        return session.session_id;
      } catch (e) {
        passwords.current.delete(connection.id);
        const error = toCommandError(e);
        if (error.kind !== "auth") {
          setToast(`${connection.name}: ${error.message}`);
          return null;
        }
        const entered = await askPassword(connection, error.message);
        if (entered === null) return null;
        password = entered;
        prompted = true;
      }
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
        const catalog = await track(t("Load the tables and columns of {name} for completion", { name: connection.name }), api.completionCatalog(sessionId));
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

  function closeSession(id: number) {
    setOpenSessions((m) => {
      const rest = new Map(m);
      rest.delete(id);
      return rest;
    });
    void api.closeSession(id);
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
        if (id !== null) closeSession(id);
      });
    }
  }

  function closeTab(tab: TabModel, confirmed = false) {
    const question = tab.unsaved
      ? t("This tab has unsaved changes. Close it and discard them?")
      : tab.txOpen
        ? t("This tab has an open transaction. Close it and roll back the uncommitted changes?")
        : null;
    if (question && !confirmed) {
      void ask(question, { confirm: t("Close"), danger: true }).then((yes) => yes && closeTab(tab, true));
      return;
    }
    // A temporary connection's tab cannot be reopened: its connection goes with its last tab.
    const saved = temporary.current.has(tab.connectionId) ? null : persistTab(tab);
    if (saved) {
      closedTabs.current.push(saved);
      if (closedTabs.current.length > CLOSED_TABS) closedTabs.current.shift();
    }
    dropSession(tab.key);
    // Closing a tab of the split leaves the other one.
    const beside = split?.find((k) => k !== tab.key && split.includes(tab.key));
    setTabs((all) => {
      const rest = all.filter((t) => t.key !== tab.key);
      // The next tab shown is one of the same project.
      const same = rest.filter((t) => inProject(t, tab.project ?? null));
      const next = same.find((t) => t.key === beside)?.key ?? same.at(-1)?.key ?? null;
      setActiveTab((active) => (active === tab.key ? next : active));
      return rest;
    });
  }

  /** Closes several tabs, asking once if some of them have changes. */
  async function closeTabs(list: TabModel[]) {
    const changed = list.filter((t) => t.unsaved || t.txOpen).length;
    const question = plural(
      changed,
      "One of these tabs has changes not saved or committed. Close it and discard them?",
      "{n} of these tabs have changes not saved or committed. Close them and discard the changes?",
    );
    if (changed > 0 && !(await ask(question, { confirm: t("Close"), danger: true }))) return;
    for (const t of list) closeTab(t, true);
  }

  /** Shows `tab` beside the active one, or a new SQL editor of the same connection. */
  function openBeside(tab: TabModel | null) {
    const active = visibleTabs.find((t) => t.key === activeTab);
    if (!active) return;
    const other = tab ?? { ...createTab("query", active.connectionId, { database: active.database }), project: activeProject };
    if (!tab) setTabs((all) => [...all, other]);
    focusedPane.current = other.key;
    setSplit([active.key, other.key]);
    setActiveTab(other.key);
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
    addTab({ ...createTab(kind.kind, saved.connectionId, kind.restore(saved)), pinned: saved.pinned === true });
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
    projectFolders,
    importOpen: dialog?.kind === "import",
    newConnection,
    closeTabs,
    patchTab,
    split,
    openBeside,
    closeSplit: () => setSplit(null),
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
    projectFolders: () => shell.current.projectFolders,
    setWorkspace: (w) => setWorkspace(w),
    activeProject: () => shell.current.projects.find((p) => p.id === shell.current.activeProject) ?? null,
    switchProject: (id) => shell.current.switchProject(id),
    editProject: (project) => setDialog({ kind: "project", project }),
    newConnection: (folder) => void shell.current.newConnection(folder ?? null),
    deleteProject: (project) => shell.current.deleteProject(project),
    openTab: (kind, connectionId, init) => shell.current.addTab(createTab(kind, connectionId, init)),
    openTemporaryTab: (kind, connection, init, onClosed) => {
      temporary.current.set(connection.id, { connection, onClosed });
      shell.current.addTab(createTab(kind, connection.id, init));
    },
    focusTab: (match) => {
      const tab = shell.current.tabs.find(match);
      if (tab) shell.current.setActiveTab(tab.key);
      return tab !== undefined;
    },
    sideSession: (connection, database) => shell.current.navigatorSession(connection, database),
    duplicateConnection: (connection) => duplicateConnection(connection),
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
  // In a connection's menu, beside Copy as URL; the palette lists it for each connection.
  useEffect(
    () =>
      registerAction({
        id: "connection.duplicate",
        scope: "navigator.connection",
        title: "Duplicate",
        group: "manage",
        order: 88,
        run: ({ connection }) => workbench().duplicateConnection(connection),
      }),
    [],
  );
  // "Add to project" lists the projects as they are.
  useEffect(() => registerAddToProject(projects ?? []), [projects]);
  useEffect(() => registerMoveToFolder(project), [project]);

  // The errors nothing caught go to the Activity log too.
  useEffect(() => {
    const unexpected = (message: string) => logError(t("Unexpected error: {message}", { message }));
    const onError = (e: ErrorEvent) => unexpected(e.message);
    const onRejection = (e: PromiseRejectionEvent) => unexpected(toCommandError(e.reason).message);
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);
    return () => {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    };
  }, []);

  // Global shortcuts, captured before the editor sees them.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isAsking()) return;
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

  const sidebarCollapsed = useSidebarCollapsed();
  const activeConnection = connectionById(visibleTabs.find((t) => t.key === activeTab)?.connectionId ?? "");
  return (
    // The frame takes the tint of the active tab's connection type.
    <div
      className={`shell${sidebarCollapsed ? " sidebar-collapsed" : ""}`}
      data-env={activeConnection?.connection_type ?? "local"}
      style={{ "--sidebar-width": `${sidebarWidth}px` } as CSSProperties}
    >
      <TopBar project={project} connection={activeConnection} />
      <ProjectRail
        projects={projects ?? []}
        active={activeProject}
        txProjects={txProjects}
        onSwitch={switchProject}
        onNew={() => setDialog({ kind: "project", project: null })}
        onMenu={(p, e) => setMenu({ x: e.clientX, y: e.clientY, target: { scope: "project", arg: p } })}
        folders={projectFolders}
        onFolderMenu={(f, e) => setMenu({ x: e.clientX, y: e.clientY, target: { scope: "project.folder", arg: f } })}
      />
      <div className="sidebar-column" hidden={sidebarCollapsed}>
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
          active={activeConnections}
          onRefresh={(c, database) => requestCatalog(c, true, database)}
          onError={setToast}
        />
        <ScriptsPanel project={project} connections={projectConnections} onError={setToast} />
        <SidebarResizer width={sidebarWidth} onResize={setSidebarWidth} />
      </div>
      <main className="workspace">
        <UpdateBanner />
        {visibleTabs.length > 0 && (
          <TabBar
            ref={tabBar}
            tabs={visibleTabs}
            activeTab={activeTab}
            besideTab={split?.find((k) => k !== activeTab) ?? null}
            grouped={grouped}
            folded={folded}
            onFold={(id) =>
              setFolded((all) => {
                const next = new Set(all);
                if (!next.delete(id)) next.add(id);
                return next;
              })
            }
            title={tabTitle}
            shortTitle={(t) => tabKind(t.kind)?.label(t) || connectionName(t.connectionId)}
            icon={tabIcon}
            color={(id) => connectionColor(connectionById(id))}
            connectionName={connectionName}
            onActivate={(t) => setActiveTab(t.key)}
            onClose={closeTab}
            onMenu={(t, x, y) => setMenu({ x, y, target: { scope: "tab", arg: { tab: t } } })}
          />
        )}
        {toast && (
          <div className="notice error toast" role="alert" onClick={() => showToast(null)}>
            {toast}
          </div>
        )}
        <div className={`panes${split ? " split" : ""}`} hidden={visibleTabs.length === 0}>
          {tabs.map((t) => {
            const connection = connectionById(t.connectionId);
            const Component = tabKind(t.kind)?.component as ComponentType<TabProps<TabModel>> | undefined;
            const pane = split ? split.indexOf(t.key) : t.key === activeTab ? 0 : -1;
            // A click or the focus in a pane of the split makes its tab the active one.
            const activate = () => split && t.key !== activeTab && setActiveTab(t.key);
            return (
              connection &&
              Component && (
                <div
                  key={t.key}
                  className={`pane${split && t.key === activeTab ? " focused" : ""}${pane === 1 ? " right" : ""}`}
                  hidden={pane < 0}
                  style={{ order: pane }}
                  onMouseDownCapture={activate}
                  onFocusCapture={activate}
                >
                  <Component
                    tab={t}
                    connection={connection}
                    active={pane >= 0}
                    ensureSession={() => ensureSession(t)}
                    currentSession={() => sessions.current.get(t.key) ?? null}
                    onChange={(patch) => patchTab(t.key, patch)}
                    sideSession={() => navigatorSession(connection, t.database)}
                    catalog={catalogs[dbKey(connection.id, t.database)] ?? null}
                    onNeedCatalog={(force) => requestCatalog(connection, force, t.database)}
                    onError={setToast}
                  />
                </div>
              )
            );
          })}
        </div>
        {visibleTabs.length === 0 && (
          <Welcome
            empty={connections?.length === 0}
            version={version}
            onNew={() => void newConnection()}
            onImport={() => setDialog({ kind: "import" })}
            onOpenFile={(path) => void openDroppedFile(path).catch((e: unknown) => setToast(toCommandError(e).message))}
            onError={setToast}
          />
        )}
      </main>

      {dialog?.kind === "connection" && (
        <ConnectionDialog
          initial={dialog.connection}
          existing={dialog.existing}
          projects={projects ?? []}
          memberOf={dialog.memberOf}
          driverChosen={dialog.driverChosen}
          onClose={() => setDialog(null)}
          onSaved={async (all) => {
            const before = connections?.find((c) => c.id === dialog.connection.id);
            await connectionsSaved(all);
            const saved = all.find((c) => c.id === dialog.connection.id);
            if (before && saved && sessionSettingsChanged(before, saved)) void followEdit(saved);
            if (dialog.folder && activeProject && saved) {
              api.placeConnection(activeProject, saved.id, dialog.folder).then(setProjects, (e: unknown) => setToast(toCommandError(e).message));
            }
          }}
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
      {menu && <ContextMenu menu={menu} onClose={() => setMenu(null)} onError={(e) => setToast(toCommandError(e).message)} />}
      {dialog?.kind === "import" && (
        <ImportDialog
          existing={connections ?? []}
          projects={projects ?? []}
          project={activeProject}
          onClose={() => setDialog(null)}
          onImported={(w) => {
            setWorkspace(w);
            setDialog(null);
          }}
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
          error={passwordRequest.error}
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
  if (t.kind === "table-editor") return "edit";
  return "tableRef" in t && t.tableRef ? "table" : "sql";
}

/** A tab as the workspace saves it, or `null` for a kind that is not kept. */
function persistTab(tab: TabModel): PersistedTab | null {
  const state = tabKind(tab.kind)?.persist(tab);
  return state ? { ...state, kind: tab.kind, connectionId: tab.connectionId, project: tab.project ?? null, ...(tab.pinned && { pinned: true }) } : null;
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
  closeTabs: (tabs: TabModel[]) => Promise<void>;
  patchTab: (key: number, patch: Partial<TabModel>) => void;
  split: [number, number] | null;
  openBeside: (tab: TabModel | null) => void;
  closeSplit: () => void;
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
      id: "tab.split",
      scope: "global",
      title: "Split editor or close the split",
      shortcut: "Mod+Shift+D",
      when: () => index() >= 0,
      // Beside the next tab, or a new SQL editor when it is the only one.
      run: () => {
        if (s().split) return s().closeSplit();
        s().openBeside(s().tabs.length > 1 ? s().tabs[(index() + 1) % s().tabs.length]! : null);
      },
    }),
    ...tabMenuActions(s),
    // `+` takes Shift on some keyboards and not on others, and the keypad has its own.
    registerAction({
      id: "app.zoomIn",
      scope: "global",
      title: "Zoom in",
      shortcut: ["Mod+=", "Mod+Plus", "Mod+Shift+Plus"],
      run: () => stepZoom(1),
    }),
    registerAction({ id: "app.zoomOut", scope: "global", title: "Zoom out", shortcut: "Mod+-", run: () => stepZoom(-1) }),
    registerAction({ id: "app.zoomReset", scope: "global", title: "Reset zoom", shortcut: "Mod+0", run: () => stepZoom(0) }),
    registerAction({
      id: "app.palette",
      scope: "global",
      title: "Command palette",
      shortcut: ["Mod+K", "Mod+Shift+P"],
      run: () => s().openPalette(),
    }),
    registerAction({ id: "app.activity", scope: "global", title: "Activity and errors", run: openActivity }),
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

/** The tab's context menu. */
function tabMenuActions(s: () => ShellApi) {
  const others = (tab: TabModel) => s().tabs.filter((t) => t.key !== tab.key && !t.pinned);
  const right = (tab: TabModel) => s().tabs.slice(s().tabs.indexOf(tab) + 1).filter((t) => !t.pinned);
  return [
    registerAction({
      id: "tab.beside",
      scope: "tab",
      title: "Open beside the active tab",
      group: "view",
      when: ({ tab }) => tab.key !== s().activeTab && !s().split?.includes(tab.key),
      run: ({ tab }) => s().openBeside(tab),
    }),
    registerAction({
      id: "tab.pin",
      scope: "tab",
      title: "Pin tab",
      group: "view",
      when: ({ tab }) => !tab.pinned,
      run: ({ tab }) => s().patchTab(tab.key, { pinned: true }),
    }),
    registerAction({
      id: "tab.unpin",
      scope: "tab",
      title: "Unpin tab",
      group: "view",
      when: ({ tab }) => !!tab.pinned,
      run: ({ tab }) => s().patchTab(tab.key, { pinned: false }),
    }),
    registerAction({ id: "tab.closeThis", scope: "tab", title: "Close", group: "close", run: ({ tab }) => s().closeTab(tab) }),
    registerAction({
      id: "tab.closeOthers",
      scope: "tab",
      title: "Close others",
      group: "close",
      when: ({ tab }) => others(tab).length > 0,
      run: ({ tab }) => s().closeTabs(others(tab)),
    }),
    registerAction({
      id: "tab.closeRight",
      scope: "tab",
      title: "Close tabs to the right",
      group: "close",
      when: ({ tab }) => right(tab).length > 0,
      run: ({ tab }) => s().closeTabs(right(tab)),
    }),
    registerAction({
      id: "tab.closeAll",
      scope: "tab",
      title: "Close all",
      group: "close",
      run: () => s().closeTabs(s().tabs.filter((t) => !t.pinned)),
    }),
  ];
}

function PasswordPrompt({
  connection,
  error,
  onDone,
}: {
  connection: SavedConnection;
  /** Why the previous password did not do: the server's answer. */
  error: string | null;
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
        <h2>{t("Password for {name}", { name: connection.name })}</h2>
        <label className="field">
          <span>
            {connection.user}@{connection.host}
          </span>
          <input autoFocus type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </label>
        {connection.save_password && !asksPassword(connection) && (
          <p className="muted small">{t("No password is saved for this connection: it will be saved in the keychain.")}</p>
        )}
        {error && (
          <div className="notice error" role="alert">
            {error}
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={() => onDone(null)}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary">
            {t("Connect")}
          </button>
        </div>
      </form>
    </div>
  );
}
