import { type ComponentType, type CSSProperties, Fragment, type ReactNode, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
import { onNavigatorRefresh, requestNavigatorRefresh, revealConnection } from "./navigatorEvents";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { ImportDialog } from "./components/ImportDialog";
import { UnlockDialog } from "./components/MasterPassword";
import { asksPassword, connectionColor, isProduction } from "./connections";
import { sessionSettingsChanged } from "./connectionEdit";
import { Navigator } from "./components/Navigator";
import { SettingsDialog } from "./components/SettingsDialog";
import { AgentContextDialog } from "./agents/AgentContextDialog";
import { followActiveTab } from "./agents/context";
import { AgentWriteDialog } from "./agents/AgentWriteDialog";
import { UpdateBanner } from "./components/UpdateBanner";
import { startupDriverCheck } from "./drivers/updates";
import { CrashBanner } from "./diagnostics/Diagnostics";
import { CommandPalette } from "./shell/CommandPalette";
import "./tabs";
import "./objects/actions";
import "./transfer/actions";
import "./schema/actions";
import "./querylog/actions";
import "./plugins/actions";
import { setAnswer } from "./testAnswers";
import { type TabKindName, type TabModel, type TabProps, createTab, tabKind } from "./tabs/registry";
import { dbKey, provideWorkbench, workbench } from "./workbench";
import type { IconName } from "./ui/Icon";
import { ask, choose, isAsking } from "./ui/ask";
import { type PersistedTab, basename, flushWorkspace, loadWorkspace, saveWorkspace } from "./workspace";
import { connectionsOf, loadActiveProject, projectKey, saveActiveProject } from "./projects/state";
import { forgetCatalog } from "./catalog/catalog";
import { ProjectRail } from "./projects/ProjectRail";
import { ProjectDialog } from "./projects/ProjectDialog";
import { AddConnectionsDialog } from "./projects/AddConnectionsDialog";
import { registerAddToProject } from "./projects/actions";
import { registerMoveToFolder } from "./projects/folders";
import { withDriver } from "./drivers/DriverPopup";
import { ScriptsPanel } from "./scripts/ScriptsPanel";
import { SavedResultsPanel } from "./analysis/SavedResults";
import { openScriptFile, scriptsChanged } from "./scripts/scripts";
import { vimTakes } from "./editor/vim";
import { ContextMenu, type MenuState } from "./components/ContextMenu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { TopBar } from "./shell/TopBar";
import { pickDataFile, Welcome } from "./shell/Welcome";
import { TabBar, barOrder, groupTabsSetting } from "./shell/TabBar";
import { SidebarResizer, useSidebarWidth } from "./shell/SidebarResizer";
import { settingsLoadError, useSetting } from "./settings";
import { useSidebarCollapsed } from "./shell/sidebar";
import { language, languageSetting, loadLanguage, t, plural } from "./i18n";
import { stepZoom } from "./theme";
import { logError, track } from "./shell/activity";
import { openActivity } from "./shell/ActivityDialog";
import { LockScreen, useIdleLock } from "./guards/AppLock";
import { PanelResizer, useStoredSize } from "./shell/PanelResizer";
import { takeLeftWindowTabs, useTray, useWindowEvents, WINDOW_EVENTS } from "./shell/windows";
import { emit, listen } from "@tauri-apps/api/event";
import { openPendingShares, openSharedItem, registerShareActions } from "./share/actions";
import { isShareFile } from "./share/shareQuery";
import { startBackground } from "./sync/background";

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
      /** Runs with the connection once it is saved. */
      afterSave?: (saved: SavedConnection) => void;
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
  // A new language shows at once, once its dictionary is loaded: everything below renders again.
  const chosenLanguage = useSetting(languageSetting);
  const [, setLanguageLoaded] = useState(0);
  useEffect(() => {
    loadLanguage().then(() => setLanguageLoaded((n) => n + 1), console.error);
  }, [chosenLanguage]);
  // The encrypted folder sync, and the folders of shared projects when the window gets focus.
  useEffect(() => startBackground(), []);
  // Downloaded drivers: once a day at most, and only when the setting is on.
  useEffect(() => {
    void startupDriverCheck();
  }, []);
  const [toast, showToast] = useState<string | null>(null);
  const [sidebarWidth, setSidebarWidth] = useSidebarWidth();
  /** Shows an error, and keeps it in the Activity log. */
  const setToast = (message: string | null) => {
    showToast(message);
    if (message) logError(message);
  };
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  /** The left half of a split editor's share of the room, per mille. */
  const [splitShare, setSplitShare] = useStoredSize("layout.split_share", 500);
  const panes = useRef<HTMLDivElement>(null);
  /** The app waits behind the master password. */
  const [locked, setLocked] = useState(false);
  /** Closed tabs as they would be saved, newest last, for Ctrl+Shift+T. */
  const closedTabs = useRef<PersistedTab[]>([]);
  const tabBar = useRef<HTMLDivElement>(null);
  /** Dialogs opened by actions through `workbench().openDialog`. */
  const [dialogs, setDialogs] = useState<{ id: number; node: ReactNode }[]>([]);
  const nextDialog = useRef(1);
  /** What each open session's driver supports. */
  const capabilities = useRef(new Map<number, Capabilities>());
  const [version, setVersion] = useState("");
  const [portable, setPortable] = useState(false);
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
      setVersion(i.build ? `${i.version} · ${i.build}` : i.version);
      setPortable(i.portable);
      // The end-to-end tests drive what has no button yet through these.
      if (i.devtools) {
        (window as unknown as { __platypus: object }).__platypus = {
          switchProject: (id: string | null) => shell.current.switchProject(id),
          // What a file dropped on the window does; CDP cannot drop files.
          openDroppedFile: (path: string) => openDroppedFile(path),
          // What the OS does with a shared query: a link or a `.platypus` file opened with the app.
          openShared: (item: string) => openSharedItem(item),
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
    // Tabs of windows the last run did not close cleanly (quit from the tray) come back here.
    const left = takeLeftWindowTabs().map((t) => {
      const kind = tabKind(t.kind ?? "query");
      return kind && known.has(t.connectionId) ? { ...createTab(kind.kind, t.connectionId, kind.restore(t)), project: null } : null;
    });
    const restoredTabs = [...byIndex, ...left].filter((t) => t !== null);
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
      const changed = tabsNow.current.filter(hasChanges).length;
      const question = plural(
        changed,
        "One tab has changes not saved or committed. Close the window and discard them?",
        "{n} tabs have changes not saved or committed. Close the window and discard them?",
      );
      if (changed > 0 && !(await ask(question, { confirm: t("Close"), danger: true }))) event.preventDefault();
    });
    // A reload, or a window closed another way, writes the last keys typed too.
    window.addEventListener("pagehide", flushWorkspace);
    return () => {
      window.removeEventListener("pagehide", flushWorkspace);
      void off.then((unlisten) => unlisten());
    };
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
    const question = p.shared
      ? t('Remove the shared project "{name}" from the app? The shared folder and its files are not deleted; its connections stay in All connections.', { name: p.name })
      : t('Delete the project "{name}"? Its connections stay, in All connections and other projects.', { name: p.name });
    if (!(await ask(question, { confirm: p.shared ? t("Remove") : t("Delete"), danger: true }))) {
      return;
    }
    const own = tabs.filter((t) => t.project === p.id);
    if (own.some(hasChanges) && !(await ask(t("Some of its tabs have changes not saved or committed. Close them and discard the changes?"), { confirm: t("Discard"), danger: true }))) {
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
    // A shared query is not a data file: it opens as a query, after choosing its connection.
    if (isShareFile(path)) return openSharedItem(path);
    // Windows paths ignore case and take either slash.
    const key = (p: string) => (/^[a-z]:/i.test(p) ? p.replace(/\//g, "\\").toLowerCase() : p);
    const saved = shell.current.connections?.find((c) => c.driver === "duckdb" && key(c.database) === key(path));
    if (saved) return workbench().openTab("query", saved.id, {});
    const connection = await api.defaultConnection("duckdb");
    const name = path.split(/[\\/]/).pop() ?? path;
    const active = shell.current.activeProject;
    setDialog({ kind: "connection", connection: { ...connection, name, database: path }, existing: false, memberOf: active ? [active] : [], driverChosen: true });
  }

  /** The sample database: its connection saved (in the active project too), its scripts
   *  listed, and the first one opened. */
  async function openSample() {
    const sample = await api.openSampleDatabase(shell.current.activeProject);
    setWorkspace(sample.workspace);
    scriptsChanged();
    await openScriptFile(sample.script, sample.connection);
  }

  /** A script dropped on the window opens in a new SQL editor of the shown tab's connection. */
  async function openDroppedScript(path: string) {
    const { tabs: shown, activeTab: key } = shell.current;
    const active = shown.find((t) => t.key === key);
    if (!active) return setToast(t("Open a connection first, then drop the script on its tab."));
    const sql = await api.readTextFile(path);
    workbench().openTab("query", active.connectionId, { sql, filePath: path, database: active.database });
  }

  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent(({ payload }) => {
      // The import dialog reads the files dropped on it.
      if (shell.current.importOpen) return;
      const dropped = payload.type === "drop" ? payload.paths : [];
      const script = dropped.find((p) => /\.sql$/i.test(p));
      if (script) return void openDroppedScript(script).catch((e: unknown) => setToast(toCommandError(e).message));
      const shared = dropped.find(isShareFile);
      if (shared) return void openSharedItem(shared);
      const path = dropped.find(isDataFile);
      if (path) void openDroppedFile(path).catch((e: unknown) => setToast(toCommandError(e).message));
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);

  // Shared queries the app was started with (a file or link given by the OS), and those a
  // second launch hands over; only the main window opens them.
  useEffect(() => {
    if (!workspaceReady || getCurrentWebview().label !== "main") return;
    void openPendingShares();
    const off = listen("share-open", () => void openPendingShares());
    return () => void off.then((unlisten) => unlisten());
  }, [workspaceReady]);

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
  function addTab(tab: TabModel, at?: number) {
    const placed = tab.project === undefined ? { ...tab, project: activeProject } : tab;
    setTabs((all) =>
      at === undefined || at < 0 || at >= all.length ? [...all, placed] : [...all.slice(0, at), placed, ...all.slice(at)],
    );
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
  /** Connections already told that their server is older than the oldest version tested. */
  const outdatedShown = useRef(new Set<string>());
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
  async function connect(connection: SavedConnection, database?: string | null, side = false): Promise<number | null> {
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
      const open = () => track(t("Connect to {name}", { name: target }), withDriver(() => api.openSession(connection.id, typed, database, side)));
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
        // Once per connection and run: each tab opens a session of its own.
        if (session.minimum && !outdatedShown.current.has(connection.id)) {
          outdatedShown.current.add(connection.id);
          setToast(
            t("{name}: {product} {version} is older than {minimum}, the oldest version tested: some features may not work.", {
              name: connection.name,
              product: session.server.product,
              version: session.server.version,
              minimum: session.minimum,
            }),
          );
        }
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
    return cached(navSessions.current, dbKey(connection.id, database), () => connect(connection, database, true));
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

  /** Asks what becomes of the open transaction of a tab that closes: committed, rolled back
   *  (as closing its session does), or the tab stays; `false` keeps it open. */
  async function endOpenTransaction(tab: TabModel): Promise<boolean> {
    const answer = await choose(t("This tab has an open transaction. Commit it or roll it back before closing?"), [
      { label: t("Commit"), value: "commit", primary: true },
      { label: t("Rollback"), value: "rollback", danger: true },
    ]);
    if (answer !== "commit") return answer === "rollback";
    try {
      const id = await sessions.current.get(tab.key);
      if (id != null) await api.endTransaction(id, true);
      return true;
    } catch (e) {
      setToast(toCommandError(e).message);
      return false;
    }
  }

  /** Asks before closing `tab` what closing would lose: grid edits, an open transaction, text
   *  not saved to its script file (which can be saved first); `false` keeps it open. */
  async function confirmClose(tab: TabModel): Promise<boolean> {
    if (tab.unsaved) {
      if (!(await ask(t("This tab has unsaved changes. Close it and discard them?"), { confirm: t("Close"), danger: true }))) return false;
    } else if (tab.txOpen && !(await endOpenTransaction(tab))) {
      return false;
    }
    const file = unsavedFile(tab);
    if (!file) return true;
    const answer = await choose(t("Save the changes to {file}?", { file: basename(file.path) }), [
      { label: t("Save"), value: "save", primary: true },
      { label: t("Don't save"), value: "discard", danger: true },
    ]);
    if (answer === null) return false;
    if (answer === "discard") return true;
    try {
      await api.writeTextFile(file.path, file.sql);
      return true;
    } catch (e) {
      setToast(toCommandError(e).message);
      return false;
    }
  }

  /** Keeps `saved` for Ctrl+Shift+T, the oldest dropped past `CLOSED_TABS`. */
  function keepClosed(saved: PersistedTab) {
    closedTabs.current.push(saved);
    if (closedTabs.current.length > CLOSED_TABS) closedTabs.current.shift();
  }

  /** Closes `tab`, asking first unless `confirmed`; kept to reopen unless `moved` to another
   *  window. */
  function closeTab(tab: TabModel, confirmed = false, moved = false) {
    if (!confirmed) {
      void confirmClose(tab).then((yes) => yes && closeTab(tab, true));
      return;
    }
    // A temporary connection's tab cannot be reopened: its connection goes with its last tab.
    const saved = temporary.current.has(tab.connectionId) || moved ? null : persistTab(tab);
    if (saved) {
      keepClosed(saved);
      closedAt.set(saved, tabs.findIndex((t) => t.key === tab.key));
    }
    // The tab next to it in the bar is shown next, as browsers do.
    const bar = visibleTabs.findIndex((t) => t.key === tab.key);
    const neighbours = bar < 0 ? [] : [visibleTabs[bar + 1], visibleTabs[bar - 1]];
    dropSession(tab.key);
    // Closing a tab of the split leaves the other one.
    const beside = split?.find((k) => k !== tab.key && split.includes(tab.key));
    setTabs((all) => {
      const rest = all.filter((t) => t.key !== tab.key);
      // The next tab shown is one of the same project.
      const same = rest.filter((t) => inProject(t, tab.project ?? null));
      const near = neighbours.find((n) => n && same.some((t) => t.key === n.key));
      const next = same.find((t) => t.key === beside)?.key ?? near?.key ?? same.at(-1)?.key ?? null;
      setActiveTab((active) => (active === tab.key ? next : active));
      return rest;
    });
  }

  /** Closes several tabs, asking once if some of them have changes. */
  async function closeTabs(list: TabModel[]) {
    const changed = list.filter(hasChanges).length;
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
    // Back in the place it had.
    addTab({ ...createTab(kind.kind, saved.connectionId, kind.restore(saved)), pinned: saved.pinned === true }, closedAt.get(saved));
  };
  const openPalette = () => setPaletteOpen(true);
  /** Moves `tab` to a window of its own; its session stays here, so an open transaction or
   *  grid changes keep it here. */
  async function detachTab(tab: TabModel) {
    if (temporary.current.has(tab.connectionId)) return setToast(t("A tab of Query with SQL stays in the main window."));
    if (tab.txOpen || tab.unsaved) return setToast(t("Commit or roll back, and save the grid's changes, before moving the tab: its session stays here."));
    const saved = persistTab(tab);
    const kind = tabKind(tab.kind);
    if (!saved || !kind) return;
    const name = connectionById(tab.connectionId)?.name ?? "";
    try {
      await api.openTabWindow(JSON.stringify(saved), `${kind.label(tab as never)} · ${name}`);
      closeTab(tab, true, true);
    } catch (e) {
      setToast(toCommandError(e).message);
    }
  }
  // What the other windows send: tabs to open here, a tab moved back, a closed window's tab.
  useWindowEvents({
    openTab: ({ kind, connectionId, init }) => connectionById(connectionId) && addTab({ ...createTab(kind, connectionId, init ?? {}), project: activeProject }),
    // Back in its project, pinned if it was.
    attach: (saved) => {
      const kind = tabKind(saved.kind ?? "query");
      if (!kind || !connectionById(saved.connectionId)) return;
      const project = saved.project && projects?.some((p) => p.id === saved.project) ? saved.project : null;
      addTab({ ...createTab(kind.kind, saved.connectionId, kind.restore(saved)), project, pinned: saved.pinned === true });
    },
    closed: keepClosed,
    unlocked: () => setLocked(false),
  });
  useTray();
  /** Locks the app behind the master password, closing the production sessions; without a
   *  master password, says what it needs. */
  async function lockApp(asked: boolean) {
    const status = await api.vaultStatus().catch(() => null);
    if (!status?.enabled) {
      if (asked) setToast(t("Locking needs a master password: set one in Settings › Master password."));
      return;
    }
    await api.lockVault().catch(() => {});
    for (const c of connections ?? []) {
      if (!isProduction(c)) continue;
      disconnectNavigator(c);
      for (const tab of tabs) {
        if (tab.connectionId !== c.id || !sessions.current.has(tab.key)) continue;
        dropSession(tab.key);
        patchTab(tab.key, { txOpen: false });
      }
    }
    setLocked(true);
    // The other windows lock too.
    void emit(WINDOW_EVENTS.locked).catch(() => {});
  }
  useIdleLock(() => void lockApp(false), locked);
  // `tabs` here are the active project's: the tab actions work on what the bar shows.
  const shellState = {
    tabs: visibleTabs,
    activeTab,
    connections,
    projectConnections,
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
    importConnections: () => setDialog({ kind: "import" }),
    openDataFile: () =>
      void pickDataFile()
        .then((path) => (path === null ? undefined : openDroppedFile(path)))
        .catch((e: unknown) => setToast(toCommandError(e).message)),
    closeTabs,
    patchTab,
    split,
    openBeside,
    closeSplit: () => setSplit(null),
    lockApp: () => void lockApp(true),
    detachTab: (tab: TabModel) => void detachTab(tab),
  };
  const shell = useRef(shellState);
  shell.current = shellState;

  // Agents the user lets see their work read the active tab.
  useEffect(() => followActiveTab(activeTab), [activeTab]);

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
    newConnectionFrom: (connection, afterSave) =>
      setDialog({
        kind: "connection",
        connection,
        existing: false,
        memberOf: shell.current.activeProject ? [shell.current.activeProject] : [],
        driverChosen: true,
        afterSave,
      }),
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
    openSession: (connection, side = false) => connect(connection, null, side),
    closeSession: (sessionId) => closeSession(sessionId),
    duplicateConnection: (connection) => duplicateConnection(connection),
    capabilities: (sessionId) => capabilities.current.get(sessionId),
    showError: setToast,
    notify: (message) => showToast(message),
    openSettings: () => setSettingsOpen(true),
    openDialog: (render) => {
      const id = nextDialog.current++;
      const close = () => setDialogs((all) => all.filter((d) => d.id !== id));
      setDialogs((all) => [...all, { id, node: render(close) }]);
    },
  });

  useLayoutEffect(() => registerShellActions(shell), []);
  useLayoutEffect(
    () =>
      registerShareActions(() => {
        const tab = shell.current.tabs.find((t) => t.key === shell.current.activeTab);
        return tab?.kind === "query" && !tab.table ? tab : null;
      }),
    [],
  );
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

  // A settings.toml that cannot be read leaves every setting at its default: say so, or the
  // user's settings look lost and every change seems to do nothing.
  useEffect(() => {
    const error = settingsLoadError();
    if (error) {
      setToast(t("settings.toml could not be read: every setting is at its default and changes are not saved until it is fixed. {message}", { message: error }));
    }
  }, []);

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

  // Global shortcuts, captured before the editor sees them. Before the first paint, like the
  // shell's actions: a key pressed as soon as the window shows is not lost.
  useLayoutEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // A dialog or the palette takes the keyboard: nothing acts behind it.
      // A Vim editor's own Ctrl keys (Ctrl+W deletes a word there).
      if (isAsking() || layerOpen() || vimTakes(e)) return;
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
    if (label) return `${label} — ${name}`;
    // Untitled editors of one connection are told apart by number: "pg", "pg (2)", "pg (3)".
    const same = visibleTabs.filter(
      (t) => t.kind === tab.kind && t.connectionId === tab.connectionId && t.database === tab.database && !tabKind(t.kind)?.label(t),
    );
    const n = same.findIndex((t) => t.key === tab.key);
    return n > 0 ? `${name} (${n + 1})` : name;
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
        <SavedResultsPanel onError={setToast} />
        <SidebarResizer width={sidebarWidth} onResize={setSidebarWidth} />
      </div>
      <main className="workspace">
        <CrashBanner />
        <UpdateBanner portable={portable} />
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
        <div className={`panes${split ? " split" : ""}`} hidden={visibleTabs.length === 0} ref={panes}>
          {split && (
            <PanelResizer
              storeKey="layout.split_share"
              size={splitShare}
              fallback={500}
              min={200}
              max={() => 800}
              direction="column"
              scale={() => (panes.current?.clientWidth ?? 1000) / 1000}
              onResize={setSplitShare}
              label={t("Resize the split editor")}
              className="split-resizer"
            />
          )}
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
                  // The left half takes its share of the room (per mille), the right the rest.
                  style={{ order: pane * 2, flexGrow: split ? (pane === 0 ? splitShare : 1000 - splitShare) : 1 }}
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
            // Translated at each render, so it follows a change of language.
            version={portable ? `${version} · ${t("portable")}` : version}
            onNew={() => void newConnection()}
            onImport={() => setDialog({ kind: "import" })}
            onOpenFile={(path) => void openDroppedFile(path).catch((e: unknown) => setToast(toCommandError(e).message))}
            onSample={openSample}
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
            // A new connection is selected in the navigator, with the focus, as the next step.
            if (!before && saved) revealConnection(saved.id);
            if (before && saved && sessionSettingsChanged(before, saved)) void followEdit(saved);
            if (saved) dialog.afterSave?.(saved);
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
      <AgentWriteDialog />
      <AgentContextDialog />
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
      {/* Over everything, dialogs included. */}
      {locked && (
        <LockScreen
          onUnlocked={() => {
            setLocked(false);
            void emit(WINDOW_EVENTS.unlocked).catch(() => {});
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
  if (t.kind === "erd" || t.kind === "schema-history") return "schema";
  if (t.kind === "table-profile") return "activity";
  return "tableRef" in t && t.tableRef ? "table" : "sql";
}

/** A tab as the workspace saves it, or `null` for a kind that is not kept. */
export function persistTab(tab: TabModel): PersistedTab | null {
  const state = tabKind(tab.kind)?.persist(tab);
  return state ? { ...state, kind: tab.kind, connectionId: tab.connectionId, project: tab.project ?? null, ...(tab.pinned && { pinned: true }) } : null;
}

/** A dialog or the command palette is shown (one left open in a hidden tab does not count). */
const layerOpen = () => [...document.querySelectorAll(".modal, .palette")].some((m) => m.getClientRects().length > 0);

/** Where each closed tab was in the list of tabs, to reopen it in its place. */
const closedAt = new WeakMap<PersistedTab, number>();

/** A SQL editor's script file and the text not saved to it yet; `null` when it has none or
 *  the file holds the text. */
function unsavedFile(tab: TabModel): { path: string; sql: string } | null {
  if (tab.kind !== "query" || !tab.filePath || tab.sql === tab.savedSql) return null;
  return { path: tab.filePath, sql: tab.sql };
}

/** Closing the tab would lose something: grid edits, an open transaction, script text. */
const hasChanges = (tab: TabModel) => !!tab.unsaved || !!tab.txOpen || unsavedFile(tab) !== null;

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
  /** The connections of the project shown. */
  projectConnections: SavedConnection[];
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
  newConnection: () => void;
  importConnections: () => void;
  openDataFile: () => void;
  lockApp: () => void;
  detachTab: (tab: TabModel) => void;
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
        // With no tab open, the first connection of the project shown.
        const connectionId = s().tabs[index()]?.connectionId ?? s().projectConnections[0]?.id;
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
      id: "app.lock",
      scope: "global",
      title: "Lock PlatypusDB",
      run: () => s().lockApp(),
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
    // The welcome page's actions, for the palette.
    registerAction({ id: "app.new_connection", scope: "global", title: "New connection…", run: () => s().newConnection() }),
    registerAction({ id: "app.import_connections", scope: "global", title: "Import connections…", run: () => s().importConnections() }),
    registerAction({ id: "app.open_data_file", scope: "global", title: "Open a data file…", run: () => s().openDataFile() }),
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
      id: "tab.window",
      scope: "tab",
      title: "Open in a new window",
      group: "view",
      run: ({ tab }) => s().detachTab(tab),
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

export function PasswordPrompt({
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
