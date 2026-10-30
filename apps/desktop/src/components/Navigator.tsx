import { type RefObject, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { formatBytes } from "../format";
import {
  api,
  type Capabilities,
  isEmbedded,
  isNoSql,
  toCommandError,
  type ObjectKind,
  type ObjectMeta,
  type Project,
  type SavedConnection,
  type SchemaMeta,
  type TableDetails,
} from "../api";
import { defineSetting, getSetting, useSetting } from "../settings";
import { dropCached, loadCached, pruneCached, saveCached } from "../navigator/diskCache";
import { FilterDialog } from "../navigator/FilterDialog";
import { ROW_HEIGHT, rowWindow, scrollToRow, stepAt } from "../navigator/rowWindow";
import { type Bookmark, isBookmarked, nameMatcher, toggleBookmark, useNavigatorPrefs } from "../navigator/prefs";
import { onNavigatorRefresh, onRevealConnection } from "../navigatorEvents";
import { toggleSidebar, toggleSidebarTitle } from "../shell/sidebar";
import { annotation, catalogEntry, catalogKey, parseFilter, useCatalogVersion } from "../catalog/catalog";
import { openDocs } from "../catalog/DocsDialog";
import { toggled } from "../sets";
import { SQL_DRAG, type SqlDrag, qualifiedName, quoteIdent, selectLimit, selectSql, viewerSql } from "../sqlNames";
import type { TabInit, TabModel } from "../tabs/registry";
import { dispatchKey, registerAction } from "../actions";
import { workbench } from "../workbench";
import { connectionColor, typeBadge } from "../connections";
import { hasUrl } from "../connectionUrl";
import { Icon, type IconName, objectIcon } from "../ui/Icon";
import { ContextMenu, type MenuState, type MenuTarget } from "./ContextMenu";
import { testConnections } from "./TestConnectionsDialog";
import { applyArrange, type FolderNode, folderTree, newFolder } from "../projects/folders";
import { type Arrange, type DragItem, type DropZone, dropChange, dropZone, listLayout } from "../projects/arrange";
import { t, msg, plural } from "../i18n";
import { openObjectTab } from "../objects/open";

/** `cached`: read from the disk cache, not yet reloaded from the server. */
type Load<T> =
  | { state: "loading" }
  | { state: "error"; message: string }
  | { state: "ok"; data: T; cached?: boolean };

/**
 * Loaded metadata by node key. A database's base key is the connection id for the saved
 * database and `cid@name` for the others; below it `base` → schemas, `base/schema` →
 * objects, `base/schema/name` → details. `cid#dbs` holds the server's databases.
 * The `ok` entries of each connection are also kept on disk (`diskCache.ts`).
 */
type Cache = Map<string, Load<unknown>>;

const viewSetting = defineSetting({
  key: "navigator.view",
  section: "Navigator",
  label: "Navigator view",
  description: "Simple shows only the tables and views of the user schemas, with their columns.",
  default: "advanced" as string,
  control: {
    type: "select",
    options: [
      { value: "advanced", label: "Advanced" },
      { value: "simple", label: "Simple" },
    ],
  },
});

const SIMPLE_KINDS = new Set<ObjectKind>(["table", "view", "materialized_view"]);

const compact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });

/** `~1.2K`: the row count the statistics say, compact, for the end of the row. */
function rowCount(object: ObjectMeta): string | undefined {
  return object.rows == null ? undefined : compact.format(object.rows);
}

/** `~1,234 rows · 3.4 MB`, for the row's tooltip. */
function sizeDetail(object: ObjectMeta): string | undefined {
  const parts = [object.rows != null && `~${plural(object.rows, "{n} row", "{n} rows")}`, object.size != null && formatBytes(object.size)];
  return parts.filter(Boolean).join(" · ") || undefined;
}

interface Props {
  connections: SavedConnection[];
  /** The active project, whose connections these are; `null` for All connections. */
  project: Project | null;
  /** Adds saved connections to the active project. */
  onAddExisting: () => void;
  error: string | null;
  onNew: () => void;
  /** Opens the import of connections from other tools. */
  onImport: () => void;
  onEdit: (c: SavedConnection) => void;
  onDelete: (c: SavedConnection) => void;
  /** Opens a new editor tab: optionally with SQL, run at once, or as a data viewer. */
  onOpenTab: (c: SavedConnection, init?: TabInit<"query">) => void;
  /** The navigator session of a connection and database (`null`: the saved one), opened
   *  on first use; `null` if cancelled. */
  session: (c: SavedConnection, database?: string | null) => Promise<number | null>;
  capabilities: (sessionId: number) => Capabilities | undefined;
  /** Drops the navigator sessions, e.g. after the connection broke. */
  disconnect: (c: SavedConnection) => void;
  /** Connections with an open session, in an editor or the navigator. */
  active: ReadonlySet<string>;
  /** A connection was refreshed: cached metadata elsewhere (autocompletion) is stale. */
  onRefresh: (c: SavedConnection, database?: string | null) => void;
  onError: (message: string) => void;
}

/** The folders of a schema; labels are translated where they are shown. */
const FOLDERS: { kind: ObjectKind; label: string }[] = [
  { kind: "table", label: msg("Tables") },
  { kind: "view", label: msg("Views") },
  { kind: "materialized_view", label: msg("Materialized views") },
  { kind: "foreign_table", label: msg("Foreign tables") },
  { kind: "sequence", label: msg("Sequences") },
  { kind: "function", label: msg("Functions") },
  { kind: "procedure", label: msg("Procedures") },
  { kind: "trigger", label: msg("Triggers") },
  { kind: "type", label: msg("Types") },
  { kind: "domain", label: msg("Domains") },
  { kind: "extension", label: msg("Extensions") },
  { kind: "event", label: msg("Events") },
  { kind: "dictionary", label: msg("Dictionaries") },
  { kind: "synonym", label: msg("Synonyms") },
  { kind: "package", label: msg("Packages") },
];


export const isRelation = (kind: ObjectKind) =>
  kind === "table" ||
  kind === "view" ||
  kind === "materialized_view" ||
  kind === "foreign_table" ||
  kind === "dictionary";

/** The cache key under which a database's schemas live. */
const baseKey = (c: SavedConnection, database: string | null) => (database === null ? c.id : `${c.id}@${database}`);

/** `key` itself or a node below it. */
const isUnder = (k: string, key: string) =>
  k === key || k.startsWith(`${key}/`) || k.startsWith(`${key}@`) || k.startsWith(`${key}#`);

interface Row {
  key: string;
  depth: number;
  label: string;
  icon?: IconName;
  color?: string | null;
  /** A short tag after the label, e.g. PROD. */
  badge?: string | null;
  detail?: string;
  /** A compact number at the end of the row, muted; it gives way before the label does. */
  count?: string;
  /** The tooltip; a function builds it for the rows rendered only. */
  title?: string | (() => string);
  /** Has a local note or tags: a discreet mark after the label. */
  marked?: boolean;
  muted?: boolean;
  /** `undefined`: leaf. */
  expanded?: boolean;
  /** A connection with an open session. */
  connected?: boolean;
  onToggle?: () => void;
  onOpen?: () => void;
  /** Its context menu lists the actions registered for this scope. */
  menu?: MenuTarget;
  actions?: React.ReactNode;
  status?: "loading" | "error";
  /** A connection or folder of the list: dragged to arrange it, and taking drops. */
  item?: DragItem;
  /** A table or column dragged into an editor. */
  sql?: SqlDrag | (() => SqlDrag);
}

const DRAG_TYPE = "application/x-platypus-navigator";

/** Whether a connection or folder of the list is being dragged. */
const isArranging = (e: React.DragEvent) => e.dataTransfer.types.includes(DRAG_TYPE);

export function Navigator(props: Props) {
  const { connections, project, onAddExisting, error, onNew, onImport, onEdit, onOpenTab, session, disconnect, active, onError } = props;
  // The ref is the source of truth, so a load sees what was merged just before it.
  const cacheRef = useRef<Cache>(new Map());
  useCatalogVersion();
  const [cache, setCache] = useState<Cache>(cacheRef.current);
  const updateCache = (change: (m: Cache) => Cache) => {
    cacheRef.current = change(cacheRef.current);
    setCache(cacheRef.current);
  };
  /** Connections that could not connect: their cached metadata is shown without retrying. */
  const offline = useRef(new Set<string>());
  const [, setOfflineVersion] = useState(0);
  const setOffline = (connectionId: string, on: boolean) => {
    if (offline.current.has(connectionId) === on) return;
    if (on) offline.current.add(connectionId);
    else offline.current.delete(connectionId);
    setOfflineVersion((v) => v + 1);
  };
  const saveTimers = useRef(new Map<string, number>());
  /** Per connection id, bumped when it closes or refreshes: loads started before are stale. */
  const generation = useRef(new Map<string, number>());
  const prefs = useNavigatorPrefs();
  const simple = useSetting(viewSetting) === "simple";
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  /** The selected rows; `anchor` is where a Shift+click range starts. */
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const anchor = useRef<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const signatures = useRef(new Map<string, string>());
  /** The connection or folder being dragged, and the row and zone it would land in. */
  const dragging = useRef<DragItem | null>(null);
  const [dropAt, setDropAt] = useState<{ key: string; zone: DropZone } | null>(null);
  /** The step of rows the tree is scrolled to: see rowWindow.ts. */
  const [step, setStep] = useState(0);

  // Forget what was loaded through a connection whose target changed.
  useEffect(() => {
    for (const c of connections) {
      const signature = JSON.stringify([c.driver, c.host, c.port, c.database, c.user, c.ssl_mode]);
      const previous = signatures.current.get(c.id);
      signatures.current.set(c.id, signature);
      if (previous !== undefined && previous !== signature) {
        closeConnection(c);
        dropCached(c.id);
      }
    }
  }, [connections]);

  useEffect(() => void pruneCached(), []);

  const setLoad = (key: string, value: Load<unknown>) => updateCache((m) => new Map(m).set(key, value));
  const loaded = <T,>(key: string) => {
    const entry = cache.get(key);
    return entry?.state === "ok" ? (entry.data as T) : null;
  };
  const isCached = (key: string) => {
    const entry = cacheRef.current.get(key);
    return entry?.state === "ok" && !!entry.cached;
  };

  /** Shows what the disk cache has for a connection not loaded yet in this run. */
  async function restoreCache(c: SavedConnection) {
    if ([...cacheRef.current.keys()].some((k) => isUnder(k, c.id))) return;
    const entries = await loadCached(c.id);
    if (entries.length === 0) return;
    updateCache((m) => {
      const next = new Map(m);
      for (const [key, data] of entries) if (!next.has(key)) next.set(key, { state: "ok", data, cached: true });
      return next;
    });
  }

  /** Writes the connection's loaded metadata to disk, once loads have settled. */
  function scheduleSave(connectionId: string) {
    clearTimeout(saveTimers.current.get(connectionId));
    const timer = window.setTimeout(() => {
      const entries: [string, unknown][] = [];
      for (const [key, entry] of cacheRef.current) {
        if (entry.state === "ok" && isUnder(key, connectionId)) entries.push([key, entry.data]);
      }
      saveCached(connectionId, entries);
    }, 1000);
    saveTimers.current.set(connectionId, timer);
  }

  /** Loads `key` from the server. Cached data stays on screen meanwhile, and when the
   *  connection fails, so the tree can be browsed offline. */
  async function load<T>(c: SavedConnection, db: string | null, key: string, fetch: (sid: number) => Promise<T>) {
    const entry = cacheRef.current.get(key);
    const cached = entry?.state === "ok" && entry.cached ? (entry.data as T) : null;
    if (cached !== null && offline.current.has(c.id)) return cached;
    if (cached === null) setLoad(key, { state: "loading" });
    // The connection closed, refreshed or changed target meanwhile: what this load brings is
    // stale and must not refill the tree nor the disk cache.
    const gen = generation.current.get(c.id) ?? 0;
    const stale = () => (generation.current.get(c.id) ?? 0) !== gen;
    const sid = await session(c, db);
    if (stale()) return null;
    if (sid === null) {
      if (cached !== null) {
        setOffline(c.id, true);
        return cached;
      }
      updateCache((m) => {
        const next = new Map(m);
        next.delete(key);
        return next;
      });
      setExpanded((s) => withoutPrefix(s, key));
      return null;
    }
    try {
      const data = await fetch(sid);
      if (stale()) return null;
      setLoad(key, { state: "ok", data });
      setOffline(c.id, false);
      scheduleSave(c.id);
      return data;
    } catch (e) {
      if (!stale()) setLoad(key, { state: "error", message: toCommandError(e).message });
      return null;
    }
  }

  /** Makes the loads of `c` under way stale. */
  const bumpGeneration = (c: SavedConnection) => generation.current.set(c.id, (generation.current.get(c.id) ?? 0) + 1);

  function loadSchemas(c: SavedConnection, db: string | null) {
    const base = baseKey(c, db);
    void load(c, db, base, (sid) => api.listSchemas(sid)).then((schemas) => {
      // Open the current schema straight away: it is almost always the one wanted.
      const current = schemas?.find((s) => s.current);
      if (current) {
        setExpanded((s) => new Set(s).add(`${base}/${current.name}`));
        loadObjects(c, db, current.name);
      }
    });
  }

  /** The server's databases, when the driver can switch between them. */
  async function loadDatabases(c: SavedConnection) {
    if (offline.current.has(c.id)) return;
    const sid = await session(c);
    if (sid === null || !props.capabilities(sid)?.multiple_databases) return;
    void load(c, null, `${c.id}#dbs`, (id) => api.listDatabases(id));
  }

  const loadObjects = (c: SavedConnection, db: string | null, schema: string) =>
    void load(c, db, `${baseKey(c, db)}/${schema}`, (sid) => api.listObjects(sid, schema));

  const loadDetails = (c: SavedConnection, db: string | null, schema: string, name: string) =>
    load(c, db, `${baseKey(c, db)}/${schema}/${name}`, (sid) => api.tableDetails(sid, schema, name));

  function toggle(key: string, onOpen?: () => void) {
    const open = !expanded.has(key);
    // Collapsing keeps the state below, so reopening shows the same nodes.
    setExpanded((s) => (open ? new Set(s).add(key) : withoutPrefix(s, key)));
    // A load that failed is tried again, e.g. a table dropped and created again since.
    if (open && (!cache.has(key) || isCached(key) || cache.get(key)?.state === "error")) onOpen?.();
  }

  /** Forgets everything loaded under `key` and reloads it if open (`open`: whether the
   *  node showing it is expanded, when that node has another key). `schemasOnly` keeps what
   *  a connection's key also holds besides its default database: the list of databases and
   *  the other databases. */
  function refresh(key: string, reload: () => void, open = expanded.has(key), schemasOnly = false) {
    const under = schemasOnly ? (k: string) => k === key || k.startsWith(`${key}/`) : (k: string) => isUnder(k, key);
    updateCache((m) => new Map([...m].filter(([k]) => !under(k))));
    if (open) reload();
  }

  /** Reloads the schemas of a database and, in place, the schemas and tables open below it
   *  (e.g. after DDL run in an editor, whose schema is not known). */
  function reloadDatabase(c: SavedConnection, db: string | null, open?: boolean) {
    const base = baseKey(c, db);
    const schemas = loaded<SchemaMeta[]>(base);
    const reopen: (() => void)[] = [];
    for (const s of schemas ?? []) {
      const skey = `${base}/${s.name}`;
      if (!expanded.has(skey)) continue;
      reopen.push(() => loadObjects(c, db, s.name));
      for (const o of loaded<ObjectMeta[]>(skey) ?? []) {
        if (isRelation(o.kind) && expanded.has(`${skey}/${o.name}`)) {
          reopen.push(() => void loadDetails(c, db, s.name, o.name));
        }
      }
    }
    // Only this database's schemas: the server's list of databases and the others stay.
    updateCache((m) => new Map([...m].filter(([k]) => k !== base && !k.startsWith(`${base}/`))));
    if (!(open ?? expanded.has(base))) return;
    if (!schemas) loadSchemas(c, db);
    else {
      void load(c, db, base, (sid) => api.listSchemas(sid));
      reopen.forEach((f) => f());
    }
  }

  async function openConnection(c: SavedConnection) {
    await restoreCache(c);
    loadSchemas(c, null);
    void loadDatabases(c);
  }

  function refreshConnection(c: SavedConnection) {
    bumpGeneration(c);
    setOffline(c.id, false);
    refresh(c.id, () => {
      loadSchemas(c, null);
      void loadDatabases(c);
    });
  }

  function closeConnection(c: SavedConnection) {
    bumpGeneration(c);
    // A pending save would overwrite the disk cache with the emptied memory cache.
    clearTimeout(saveTimers.current.get(c.id));
    updateCache((m) => new Map([...m].filter(([k]) => !isUnder(k, c.id))));
    setOffline(c.id, false);
    setExpanded((s) => withoutPrefix(s, c.id, true));
    disconnect(c);
  }

  /** Opens a table's data, or goes back to the tab already showing it. */
  function openData(c: SavedConnection, db: string | null, schema: SchemaMeta, object: ObjectMeta, target: string) {
    const showing = (t: TabModel) =>
      t.kind === "query" && t.connectionId === c.id && (t.database ?? null) === db && t.table === target;
    if (workbench().focusTab(showing)) return;
    onOpenTab(c, {
      sql: viewerSql(c.driver, target),
      autorun: true,
      title: object.name,
      table: target,
      tableRef: { schema: schema.name, name: object.name, kind: object.kind },
      database: db,
    });
  }

  function openObject(c: SavedConnection, db: string | null, schema: SchemaMeta, object: ObjectMeta) {
    openObjectTab(c.id, {
      database: db,
      schema: schema.name,
      currentSchema: schema.current,
      name: object.name,
      kind: object.kind,
      parent: object.parent ?? null,
    });
  }


  // What the navigator's own actions need, refreshed every render.
  const nav = useRef<NavigatorApi>(null!);
  nav.current = {
    props,
    refresh,
    refreshConnection,
    reloadDatabase,
    loadSchemas,
    loadObjects,
    loadDetails,
    closeConnection,
    openData,
    openObject,
    isOpen: (key) => expanded.has(key),
  };
  useEffect(() => registerNavigatorActions(nav), []);
  const closeMenu = useCallback(() => setMenu(null), []);

  // Objects changed elsewhere (DDL from an action or a tab): reload that part of the tree.
  useEffect(
    () =>
      onNavigatorRefresh(({ connectionId, database, schema }) => {
        const n = nav.current;
        const c = n.props.connections.find((x) => x.id === connectionId);
        if (!c) return;
        const db = database ?? null;
        const base = baseKey(c, db);
        // Another database's schemas show under its database node.
        const open = db === null ? undefined : n.isOpen(`${c.id}#db:${db}`);
        if (schema === undefined) n.reloadDatabase(c, db, open);
        else n.refresh(`${base}/${schema}`, () => n.loadObjects(c, db, schema));
      }),
    [],
  );

  const parsed = parseFilter(filter);
  const needle = parsed.text.toLowerCase();
  const needTags = parsed.tags;
  const rows: Row[] = [];
  const status = (key: string): Pick<Row, "status" | "detail"> => {
    const entry = cache.get(key);
    if (entry?.state === "loading") return { status: "loading", detail: t("loading…") };
    if (entry?.state === "error") return { status: "error", detail: entry.message };
    return {};
  };

  /** The rows of a connection and what is open below it. */
  function connectionRows(c: SavedConnection) {
    const ckey = c.id;
    const open = expanded.has(ckey);
    const filtered = !!prefs.filters[c.id];
    const isOffline = offline.current.has(c.id);
    const target = isEmbedded(c.driver) ? c.database : `${c.user}@${c.host}:${c.port ?? ""}/${c.database}`;
    rows.push({
      key: ckey,
      depth: 0,
      label: c.name,
      color: connectionColor(c),
      badge: typeBadge(c),
      detail: [isOffline && t("offline"), filtered && t("filtered")].filter(Boolean).join(" · ") || undefined,
      title: isOffline
        ? `${target}\n${t("Not connected: showing the metadata cached on disk. Refresh to retry.")}`
        : `${target}${active.has(c.id) ? `\n${t("Connected")}` : ""}`,
      expanded: open,
      connected: active.has(c.id),
      onToggle: () => toggle(ckey, () => void openConnection(c)),
      onOpen: () => onOpenTab(c),
      // Loading shows in the row below it; a failure marks the connection too.
      ...(status(ckey).status === "error" ? status(ckey) : {}),
      menu: { scope: "navigator.connection", arg: { connection: c } },
      item: { kind: "connection", id: c.id },
      actions: (
        <>
          <button className="icon" title={t("Open SQL editor")} aria-label={t("Open SQL editor")} onClick={() => onOpenTab(c)}>
            <Icon name="sql" />
          </button>
          <button className="icon" title={t("Edit")} aria-label={t("Edit")} onClick={() => onEdit(c)}>
            <Icon name="edit" />
          </button>
        </>
      ),
    });
    if (!open) return;
    rows.push(...bookmarkRows(c));
    const databases = loaded<string[]>(`${c.id}#dbs`);
    if (!databases || databases.length < 2) {
      rows.push(...schemaRows(c, null, 1));
      return;
    }
    // One node per database; the saved one starts open, `!` remembers it was closed.
    for (const name of databases) {
      const db = name === c.database ? null : name;
      const dkey = `${c.id}#db:${name}`;
      const base = baseKey(c, db);
      const dOpen = db === null ? !expanded.has(`${dkey}!`) : expanded.has(dkey);
      rows.push({
        key: dkey,
        depth: 1,
        label: name,
        icon: "database",
        detail: db === null ? t("default") : undefined,
        expanded: dOpen,
        onToggle: () => {
          setExpanded((s) => {
            const next = new Set(s);
            const flag = db === null ? `${dkey}!` : dkey;
            if (next.has(flag)) next.delete(flag);
            else next.add(flag);
            return next;
          });
          if (!dOpen && !cache.has(base)) loadSchemas(c, db);
        },
        onOpen: () => onOpenTab(c, { database: db }),
        ...status(db === null ? `${c.id}#dbs` : base),
        menu: { scope: "navigator.database", arg: { connection: c, database: db, name } },
      });
      if (dOpen) rows.push(...schemaRows(c, db, 2));
    }
  }

  /** A connection's rows, `level` deeper for the folders it is in. */
  function addConnection(c: SavedConnection, level: number) {
    const start = rows.length;
    connectionRows(c);
    for (let i = start; i < rows.length; i++) rows[i] = { ...rows[i]!, depth: rows[i]!.depth + level };
  }

  function addFolder(p: Project, node: FolderNode, level: number) {
    const fkey = `folder:${node.folder.id}`;
    // Folders start open; `!` remembers one was closed.
    const fOpen = !expanded.has(`${fkey}!`) || !!needle;
    rows.push({
      key: fkey,
      depth: level,
      label: node.folder.name,
      icon: fOpen ? "open" : "folder",
      expanded: fOpen,
      onToggle: () => setExpanded((s) => toggled(s, `${fkey}!`)),
      menu: { scope: "navigator.folder", arg: { project: p, folder: node.folder } },
      item: { kind: "folder", id: node.folder.id },
    });
    if (!fOpen) return;
    for (const child of node.children) addFolder(p, child, level + 1);
    for (const c of node.connections) addConnection(c, level + 1);
  }

  /** What the list arranges: the project, or the saved list for All connections. */
  const layout = project ?? listLayout(connections);

  /** Where the dragged connection or folder lands on `row` at the pointer, and the change it
   *  makes there; `null` where it cannot go or would not move. */
  function rowDrop(row: Row, e: React.DragEvent): { zone: DropZone; change: Arrange } | null {
    const item = dragging.current;
    if (!item || !row.item) return null;
    const r = e.currentTarget.getBoundingClientRect();
    const zone = dropZone(e.clientY - r.top, r.height, item.kind, row.item.kind, project !== null);
    const change = dropChange(layout, item, row.item, zone);
    return change && { zone, change };
  }

  /** Shows where a drop would land: on `key` in `zone`, or nowhere. */
  function showDrop(key: string | null, zone: DropZone = "into") {
    setDropAt((at) => (key === null ? null : at?.key === key && at.zone === zone ? at : { key, zone }));
  }

  function arrange(change: Arrange | null) {
    showDrop(null);
    dragging.current = null;
    if (change) applyArrange(project, change).catch((e: unknown) => onError(toCommandError(e).message));
  }

  /** A row taking a dragged connection or folder; one that takes nothing refuses it, so
   *  the drop does not reach the list below it. */
  const rowDropHandlers = (row: Row) => ({
    onDragOver: (e: React.DragEvent) => {
      if (!isArranging(e)) return;
      e.stopPropagation();
      const drop = rowDrop(row, e);
      showDrop(drop && row.key, drop?.zone);
      if (drop) e.preventDefault();
    },
    onDrop: (e: React.DragEvent) => {
      if (!isArranging(e)) return;
      e.preventDefault();
      e.stopPropagation();
      arrange(rowDrop(row, e)?.change ?? null);
    },
  });

  /** Below the rows: last at the root. */
  const listDrop = () => (dragging.current ? dropChange(layout, dragging.current, null, "into") : null);

  if (project) {
    const tree = folderTree(project, connections);
    for (const node of tree.folders) addFolder(project, node, 0);
    for (const c of tree.loose) addConnection(c, 0);
  } else {
    for (const c of connections) addConnection(c, 0);
  }

  function schemaRows(c: SavedConnection, db: string | null, depth: number): Row[] {
    const out: Row[] = [];
    const base = baseKey(c, db);
    const schemas = loaded<SchemaMeta[]>(base);
    if (!schemas) {
      const s = status(base);
      // The text is the row's label: not repeated as its detail.
      if (s.status) out.push({ key: `${base}#status`, depth, label: s.detail ?? "", muted: true, status: s.status });
      return out;
    }
    const hidden = prefs.filters[c.id];
    const hiddenSchema = nameMatcher(hidden?.schemas);
    const hiddenObject = nameMatcher(hidden?.objects);
    // User schemas first, catalog schemas after them.
    const ordered = schemas
      .filter((s) => !(simple && s.system) && !hiddenSchema(s.name))
      .sort((a, b) => Number(a.system) - Number(b.system));
    for (const schema of ordered) {
      const skey = `${base}/${schema.name}`;
      const objects = loaded<ObjectMeta[]>(skey);
      const matches = objects?.filter(
        (o) =>
          (!needle || o.name.toLowerCase().includes(needle)) &&
          (needTags.length === 0 ||
            needTags.every((t) => catalogEntry(c.id, catalogKey(db, schema.name, o.name))?.tags.includes(t))) &&
          (!simple || SIMPLE_KINDS.has(o.kind)) &&
          !hiddenObject(o.name),
      );
      if ((needle || needTags.length > 0) && objects && matches?.length === 0) continue;
      const sOpen = expanded.has(skey) || ((!!needle || needTags.length > 0) && !!matches?.length);
      out.push({
        key: skey,
        depth,
        label: schema.name,
        icon: "schema",
        muted: schema.system,
        expanded: sOpen,
        onToggle: () => toggle(skey, () => loadObjects(c, db, schema.name)),
        ...status(skey),
        menu: { scope: "navigator.schema", arg: { connection: c, database: db, schema } },
      });
      if (!sOpen || !matches) continue;
      if (matches.length === 0) {
        out.push({ key: `${skey}#empty`, depth: depth + 1, label: t("No objects"), muted: true });
        continue;
      }
      // Partitions show under their table, when it is listed too.
      const tables = new Set(matches.filter((o) => o.kind === "table").map((o) => o.name));
      const isPartition = (o: ObjectMeta) => o.kind === "table" && o.parent !== null && tables.has(o.parent);
      const partitions = new Map<string, ObjectMeta[]>();
      for (const o of matches) {
        if (!isPartition(o)) continue;
        const siblings = partitions.get(o.parent!);
        if (siblings) siblings.push(o);
        else partitions.set(o.parent!, [o]);
      }
      for (const folder of FOLDERS) {
        const items = matches.filter((o) => o.kind === folder.kind && !isPartition(o));
        if (items.length === 0) continue;
        const fkey = `${skey}#${folder.kind}`;
        const fOpen =
          expanded.has(fkey) || !!needle || needTags.length > 0 || (folder.kind === "table" && !expanded.has(`${fkey}!`));
        out.push({
          key: fkey,
          depth: depth + 1,
          label: t(folder.label),
          detail: String(items.length),
          expanded: fOpen,
          // Tables start open; `!` remembers that the user closed them.
          onToggle: () =>
            setExpanded((s) => {
              const next = new Set(s);
              if (fOpen) {
                next.delete(fkey);
                if (folder.kind === "table") next.add(`${fkey}!`);
              } else {
                next.add(fkey);
                next.delete(`${fkey}!`);
              }
              return next;
            }),
        });
        if (!fOpen) continue;
        for (const object of items) {
          out.push(...objectRows(c, db, schema, object, partitions, depth + 2));
        }
      }
    }
    return out;
  }

  /** The connection's bookmarks, in a folder that starts open (`!` remembers it was closed). */
  function bookmarkRows(c: SavedConnection): Row[] {
    const marks = prefs.bookmarks.filter((b) => b.connectionId === c.id);
    if (marks.length === 0) return [];
    const fkey = `${c.id}#bookmarks`;
    const fOpen = !expanded.has(`${fkey}!`);
    const out: Row[] = [
      {
        key: fkey,
        depth: 1,
        label: t("Bookmarks"),
        icon: "bookmark",
        detail: String(marks.length),
        expanded: fOpen,
        onToggle: () => setExpanded((s) => (fOpen ? new Set(s).add(`${fkey}!`) : withoutPrefix(s, `${fkey}!`))),
      },
    ];
    if (!fOpen) return out;
    for (const b of marks) {
      const schema: SchemaMeta = { name: b.schema, system: false, current: b.currentSchema };
      const object: ObjectMeta = { name: b.name, kind: b.kind, comment: null, parent: b.parent, rows: null, size: null };
      const target = qualifiedName(c.driver, schema, b.name);
      out.push({
        key: `${fkey}/${b.database ?? ""}/${b.schema}/${b.kind}/${b.parent ?? ""}/${b.name}`,
        depth: 2,
        label: b.name,
        icon: objectIcon(b.kind),
        detail: [b.database, b.schema].filter(Boolean).join(" · "),
        onOpen: () =>
          isRelation(b.kind) ? openData(c, b.database, schema, object, target) : openObject(c, b.database, schema, object),
        menu: { scope: "navigator.object", arg: { connection: c, database: b.database, schema, object, target } },
      });
    }
    return out;
  }

  function objectRows(
    c: SavedConnection,
    db: string | null,
    schema: SchemaMeta,
    object: ObjectMeta,
    partitions: Map<string, ObjectMeta[]>,
    depth: number,
  ): Row[] {
    const icon = objectIcon(object.kind);
    const entry = catalogEntry(c.id, catalogKey(db, schema.name, object.name));
    const marked = !!(entry?.note || entry?.tags.length);
    const title = annotation(entry, object.comment);
    // A relation's key is that of its details. Other kinds can share a name with a relation
    // or each other (a MySQL function and procedure): their key adds the kind, and triggers,
    // named per table in PostgreSQL, their table.
    const okey = `${baseKey(c, db)}/${schema.name}/${object.name}`;
    const target = qualifiedName(c.driver, schema, object.name);
    const menu: MenuTarget = { scope: "navigator.object", arg: { connection: c, database: db, schema, object, target } };
    if (!isRelation(object.kind)) {
      return [
        {
          key: `${okey}~${object.kind}${object.kind === "trigger" && object.parent ? `~${object.parent}` : ""}`,
          depth,
          label: object.name,
          icon,
          detail: object.parent ? t("on {table}", { table: object.parent }) : undefined,
          title: title && `${object.name}\n${title}`,
          marked,
          onOpen: () => openObject(c, db, schema, object),
          menu,
          sql: { name: target },
        },
      ];
    }
    const open = expanded.has(okey);
    const out: Row[] = [
      {
        key: okey,
        depth,
        label: object.name,
        icon,
        count: rowCount(object),
        // The full name first: a long one is cut in the row. Built for the rows rendered only.
        title: () => [object.name, title, sizeDetail(object)].filter(Boolean).join("\n"),
        marked,
        expanded: open,
        onToggle: () => toggle(okey, () => void loadDetails(c, db, schema.name, object.name)),
        onOpen: () => openData(c, db, schema, object, target),
        ...status(okey),
        menu,
        // Built when dragged: most rows never are.
        sql: () => ({ name: target, select: selectSql(c.driver, target, null, getSetting(selectLimit)) }),
      },
    ];
    const details = open ? loaded<TableDetails>(okey) : null;
    if (!details) return out;
    const section = (name: string, count: number, children: Row[]) => {
      if (count === 0 || (simple && name !== "Columns" && name !== "Partitions")) return;
      const key = `${okey}#${name}`;
      // Columns start open, the other sections closed.
      const isOpen = name === "Columns" ? !expanded.has(`${key}!`) : expanded.has(key);
      out.push({
        key,
        depth: depth + 1,
        label: t(name),
        detail: String(count),
        expanded: isOpen,
        onToggle: () =>
          setExpanded((s) => {
            const next = new Set(s);
            const flag = name === "Columns" ? `${key}!` : key;
            if (next.has(flag)) next.delete(flag);
            else next.add(flag);
            return next;
          }),
      });
      if (isOpen) out.push(...children);
    };
    section(
      msg("Columns"),
      details.columns.length,
      details.columns.map((col) => {
        const centry = catalogEntry(c.id, catalogKey(db, schema.name, object.name, col.name));
        return {
          key: `${okey}.${col.name}`,
          depth: depth + 2,
          label: col.name,
          icon: col.primary_key ? "key" : "column",
          detail: `${col.data_type}${col.nullable ? "" : ` · ${t("not null")}`}`,
          title:
            [col.default && t("default {value}", { value: col.default }), annotation(centry, col.comment)]
              .filter(Boolean)
              .join("\n") || undefined,
          marked: !!(centry?.note || centry?.tags.length),
          menu: {
            scope: "navigator.column",
            arg: { connection: c, database: db, schema, object, column: col.name },
          },
          sql: { name: quoteIdent(col.name, c.driver) },
        };
      }),
    );
    section(
      msg("Indexes"),
      details.indexes.length,
      details.indexes.map((ix) => ({
        key: `${okey}:ix:${ix.name}`,
        depth: depth + 2,
        label: ix.name,
        icon: ix.primary ? "key" : "index",
        detail: `(${ix.columns.join(", ")})${ix.primary ? ` · ${t("primary")}` : ix.unique ? ` · ${t("unique")}` : ""}${ix.size != null ? ` · ${formatBytes(ix.size)}` : ""}`,
      })),
    );
    const parts = partitions.get(object.name) ?? [];
    section(
      msg("Partitions"),
      parts.length,
      parts.flatMap((p) => objectRows(c, db, schema, p, partitions, depth + 2)),
    );
    section(
      msg("Foreign keys"),
      details.foreign_keys.length,
      details.foreign_keys.map((fk) => ({
        key: `${okey}:fk:${fk.name}`,
        depth: depth + 2,
        label: fk.name,
        icon: "fk-out",
        detail: `(${fk.columns.join(", ")}) → ${fk.ref_table}(${fk.ref_columns.join(", ")})`,
      })),
    );
    section(
      msg("References"),
      details.referenced_by.length,
      details.referenced_by.map((r) => ({
        key: `${okey}:ref:${r.schema}.${r.table}.${r.name}`,
        depth: depth + 2,
        label: r.schema === schema.name ? r.table : `${r.schema}.${r.table}`,
        icon: "fk-in",
        detail: `(${r.columns.join(", ")}) → (${r.ref_columns.join(", ")})`,
        title: r.name,
      })),
    );
    return out;
  }

  /** A click selects the row; Ctrl adds or removes it, Shift selects the range from the
   *  last clicked row. */
  function select(key: string, e: React.MouseEvent) {
    const keys = rows.map((r) => r.key);
    const from = anchor.current === null ? -1 : keys.indexOf(anchor.current);
    if (e.shiftKey && from >= 0) {
      const to = keys.indexOf(key);
      setSelected(new Set(keys.slice(Math.min(from, to), Math.max(from, to) + 1)));
      return;
    }
    anchor.current = key;
    setSelected(e.ctrlKey || e.metaKey ? toggled(selected, key) : new Set([key]));
  }

  /** The row the keys move from: the one clicked or reached last, if still shown. */
  const current = anchor.current !== null && rows.some((r) => r.key === anchor.current) ? anchor.current : null;

  // A row moved by a shortcut loses the focus when React moves its element: give it back,
  // shortly after the key only, so a later click elsewhere keeps its own. A connection just
  // made gets it the same way once its row shows, when its dialog has left the focus nowhere.
  const moved = useRef<{ key: string; at: number } | null>(null);
  const treeRef = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!moved.current || document.activeElement !== document.body) return;
    const { key, at } = moved.current;
    const index = rows.findIndex((r) => r.key === key);
    if (index < 0 && Date.now() - at <= 2000) return;
    moved.current = null;
    if (Date.now() - at > 2000) return;
    const li = treeRef.current?.querySelector<HTMLElement>(`[data-row="${index}"]`);
    if (li) li.focus();
    else reveal(index);
  });
  useEffect(
    () =>
      onRevealConnection((id) => {
        anchor.current = id;
        setSelected(new Set([id]));
        moved.current = { key: id, at: Date.now() };
      }),
    [],
  );

  /** A row reached outside the rendered ones: focused once rendered. */
  const focusing = useRef<string | null>(null);
  /** Scrolls to row `index`, which is not rendered, and focuses it once it is. */
  function reveal(index: number) {
    const tree = treeRef.current;
    if (!tree || !rows[index]) return;
    tree.scrollTop = scrollToRow(index, tree.scrollTop, tree.clientHeight);
    setStep(stepAt(tree.scrollTop));
    focusing.current = rows[index].key;
  }
  /** A row has the focus: when scrolling takes it out of the rendered rows, the tree keeps
   *  the focus, so the keys still move from the current row. */
  const rowFocused = useRef(false);
  useLayoutEffect(() => {
    if (focusing.current !== null) {
      const index = rows.findIndex((r) => r.key === focusing.current);
      focusing.current = null;
      treeRef.current?.querySelector<HTMLElement>(`[data-row="${index}"]`)?.focus();
    } else if (rowFocused.current && document.activeElement === document.body) {
      treeRef.current?.focus({ preventScroll: true });
    }
  });

  const { from, to } = rowWindow(step, rows.length);
  /** The Tab stop: the current row; the tree itself when that row is not rendered, so the
   *  keys move from it; the first row rendered when there is none. */
  const currentIndex = current === null ? -1 : rows.findIndex((r) => r.key === current);
  const currentHidden = currentIndex >= 0 && (currentIndex < from || currentIndex >= to);
  const tabStop = currentHidden ? null : (current ?? rows[from]?.key);

  /** The tree from the keyboard: arrows move and open or close, Enter opens, the menu key
   *  shows the row's menu, as in a file explorer. */
  function onTreeKey(e: React.KeyboardEvent<HTMLUListElement>) {
    if (!(e.target instanceof HTMLLIElement || e.target === e.currentTarget) || rows.length === 0) return;
    const at = Math.max(0, currentIndex);
    const row = rows[at]!;
    // The row's own shortcuts first, e.g. Alt+Up to move a connection.
    if (row.menu && dispatchKey(row.menu.scope, row.menu.arg, e.nativeEvent, (err) => onError(toCommandError(err).message))) {
      moved.current = { key: row.key, at: Date.now() };
      return;
    }
    const go = (i: number) => {
      const index = Math.max(0, Math.min(rows.length - 1, i));
      anchor.current = rows[index]!.key;
      setSelected(new Set([rows[index]!.key]));
      const li = e.currentTarget.querySelector<HTMLElement>(`[data-row="${index}"]`);
      if (li) {
        li.focus();
        li.scrollIntoView({ block: "nearest" });
        return;
      }
      // Outside the rendered rows, e.g. End in a long tree.
      reveal(index);
    };
    const parent = () => {
      for (let i = at - 1; i >= 0; i--) if (rows[i]!.depth < row.depth) return i;
      return at;
    };
    if (e.key === "ArrowDown") go(at + 1);
    else if (e.key === "ArrowUp") go(at - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(rows.length - 1);
    else if (e.key === "ArrowRight") {
      if (row.expanded === false) row.onToggle?.();
      else if (row.expanded) go(at + 1);
    } else if (e.key === "ArrowLeft") {
      if (row.expanded) row.onToggle?.();
      else go(parent());
    } else if (e.key === "Enter") {
      if (row.onOpen) row.onOpen();
      else row.onToggle?.();
    } else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      const target = menuOf(row);
      const r = (e.target as HTMLElement).getBoundingClientRect();
      if (target) setMenu({ x: r.left + 24, y: r.bottom, target });
    } else return;
    e.preventDefault();
  }

  /** The menu of `row`: with several objects selected, their actions get them all. */
  function menuOf(row: Row): MenuTarget | undefined {
    const target = row.menu;
    if (!selected.has(row.key)) {
      anchor.current = row.key;
      setSelected(new Set([row.key]));
      return target;
    }
    if (target?.scope !== "navigator.object") return target;
    const selection = rows.flatMap((r) =>
      selected.has(r.key) && r.menu?.scope === "navigator.object" ? [r.menu.arg] : [],
    );
    return selection.length > 1 ? { scope: target.scope, arg: { ...target.arg, selection } } : target;
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">
          {project ? (
            <span className="navigator-project" title={project.name}>
              <span className="dot" style={{ background: project.color }} />
              {project.name}
            </span>
          ) : (
            t("Connections")
          )}
        </span>
        <button className="icon" title={t("Import connections from DBeaver, DataGrip, pgAdmin…")} aria-label={t("Import connections")} onClick={onImport}>
          <Icon name="import" />
        </button>
        {project && (
          <button className="icon" title={t("New folder")} aria-label={t("New folder")} onClick={() => newFolder(project, null)}>
            <Icon name="new-folder" />
          </button>
        )}
        <button className="icon" title={t("New connection")} aria-label={t("New connection")} onClick={onNew}>
          <Icon name="plus" />
        </button>
        <button
          type="button"
          className="icon"
          title={toggleSidebarTitle()}
          aria-label={t("Show or hide the navigator")}
          // Shown with the navigator only.
          aria-expanded
          onClick={toggleSidebar}
        >
          <Icon name="sidebar-close" />
        </button>
      </div>
      {connections.length > 0 && (
        <div className="tree-search">
          <Icon name="search" className="tree-search-icon" />
          <input
            className="tree-filter"
            placeholder={t("Filter objects…")}
            aria-label={t("Filter objects")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setFilter("")}
          />
        </div>
      )}
      {error && <div className="notice error small">{error}</div>}
      {project && connections.length === 0 && !error && (
        <div className="project-empty">
          <p className="muted small">{t("No connections in this project.")}</p>
          <button onClick={onNew}>{t("New connection")}</button>
          <button onClick={onAddExisting}>{t("Add existing…")}</button>
          <button onClick={onImport}>{t("Import…")}</button>
        </div>
      )}
      {!project && connections.length === 0 && !error && (
        <>
          <button className="empty-cta" onClick={onNew}>
            {t("Add your first connection")}
          </button>
          <button className="link small empty-import" onClick={onImport}>
            {t("or import them from DBeaver, DataGrip, pgAdmin…")}
          </button>
        </>
      )}
      <ul
        ref={treeRef}
        className="tree"
        role="tree"
        aria-label={t("Connections")}
        aria-multiselectable="true"
        tabIndex={currentHidden ? 0 : -1}
        onKeyDown={onTreeKey}
        onFocus={(e) => (rowFocused.current = e.target instanceof HTMLLIElement)}
        onBlur={(e) => {
          // A row that scrolling unmounts leaves without one.
          if (e.relatedTarget) rowFocused.current = false;
        }}
        onScroll={(e) => setStep(stepAt(e.currentTarget.scrollTop))}
        // Below the rows: the list's menu.
        onContextMenu={(e) => {
          if (e.defaultPrevented || (e.target as HTMLElement).closest(".tree-row")) return;
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, target: { scope: "navigator.list", arg: { project } } });
        }}
        // Dropped below the rows: last at the root.
        onDragOver={(e) => {
          if (!isArranging(e)) return;
          showDrop(null);
          if (listDrop()) e.preventDefault();
        }}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) showDrop(null);
        }}
        onDrop={(e) => {
          if (!isArranging(e)) return;
          e.preventDefault();
          arrange(listDrop());
        }}
      >
        {/* The rows not rendered keep their place, so the scrollbar spans the whole tree. */}
        {from > 0 && <li role="none" style={{ height: from * ROW_HEIGHT }} />}
        {rows.slice(from, to).map((row, i) => (
          <li
            key={row.key}
            data-row={from + i}
            role="treeitem"
            aria-expanded={row.expanded}
            aria-level={row.depth + 1}
            aria-selected={selected.has(row.key)}
            tabIndex={row.key === tabStop ? 0 : -1}
            className={`tree-row${row.muted ? " muted" : ""}${row.status === "error" ? " failed" : ""}${selected.has(row.key) ? " selected" : ""}${dropAt?.key === row.key ? ` drop-${dropAt.zone}` : ""}`}
            style={{ paddingLeft: 6 + row.depth * 16 }}
            draggable={!!row.item || !!row.sql}
            onDragStart={(e) => {
              dragging.current = row.item ?? null;
              if (row.item) {
                e.dataTransfer.setData(DRAG_TYPE, JSON.stringify(row.item));
                e.dataTransfer.effectAllowed = "move";
              }
              const sql = typeof row.sql === "function" ? row.sql() : row.sql;
              if (sql) {
                e.dataTransfer.setData(SQL_DRAG, JSON.stringify(sql));
                // Anywhere else that takes text, e.g. a filter box, gets the name.
                e.dataTransfer.setData("text/plain", sql.name);
                e.dataTransfer.effectAllowed = "copy";
              }
            }}
            onDragEnd={() => {
              dragging.current = null;
              showDrop(null);
            }}
            {...rowDropHandlers(row)}
            title={
              row.status === "error"
                ? (row.detail ?? row.label)
                : ((typeof row.title === "function" ? row.title() : row.title) ??
                  (row.detail ? `${row.label}  ${row.detail}` : undefined))
            }
            onClick={(e) => select(row.key, e)}
            onDoubleClick={(e) => {
              e.preventDefault();
              // Only rows with nothing to open (schemas, folders) expand on a double-click.
              if (row.onOpen) row.onOpen();
              else row.onToggle?.();
            }}
            onContextMenu={(e) => {
              const target = menuOf(row);
              if (!target) return;
              e.preventDefault();
              setMenu({ x: e.clientX, y: e.clientY, target });
            }}
          >
            <span
              className="twisty"
              onClick={(e) => {
                if (!row.onToggle) return;
                e.stopPropagation();
                row.onToggle();
              }}
              onDoubleClick={(e) => row.onToggle && e.stopPropagation()}
            >
              {row.expanded !== undefined && <Icon name={row.expanded ? "chevron-down" : "chevron-right"} size={12} />}
            </span>
            {row.color !== undefined ? (
              <span className="dot" style={{ background: row.color ?? "var(--border)" }} />
            ) : (
              row.icon && (
                <span className="tree-icon" data-kind={row.icon}>
                  <Icon name={row.icon} />
                </span>
              )
            )}
            <span className="tree-label">{row.label}</span>
            {row.marked && <span className="catalog-mark" aria-label={t("Has notes")} />}
            {row.connected && <span className="connected-mark" role="img" aria-label={t("Connected")} />}
            {row.badge && <span className={`type-badge ${row.badge.toLowerCase()}`}>{row.badge}</span>}
            {row.detail && <span className="tree-detail">{row.detail}</span>}
            {row.count && <span className="tree-count">{row.count}</span>}
            {row.actions && (
              <span className="tree-actions" onClick={(e) => e.stopPropagation()}>
                {row.actions}
              </span>
            )}
          </li>
        ))}
        {to < rows.length && <li role="none" style={{ height: (rows.length - to) * ROW_HEIGHT }} />}
      </ul>
      {menu && (
        <ContextMenu menu={menu} onClose={closeMenu} onError={(e) => onError(toCommandError(e).message)} />
      )}
    </aside>
  );
}

/** `set` without `key` and, when `children`, without the keys below it. */
function withoutPrefix(set: Set<string>, key: string, children = false): Set<string> {
  return new Set(
    [...set].filter((k) => k !== key && !(children && isUnder(k, key))),
  );
}

interface NavigatorApi {
  props: Props;
  refresh: (key: string, reload: () => void, open?: boolean, schemasOnly?: boolean) => void;
  refreshConnection: (c: SavedConnection) => void;
  reloadDatabase: (c: SavedConnection, db: string | null, open?: boolean) => void;
  loadSchemas: (c: SavedConnection, db: string | null) => void;
  loadObjects: (c: SavedConnection, db: string | null, schema: string) => void;
  loadDetails: (c: SavedConnection, db: string | null, schema: string, name: string) => Promise<TableDetails | null>;
  closeConnection: (c: SavedConnection) => void;
  openData: (c: SavedConnection, db: string | null, schema: SchemaMeta, object: ObjectMeta, target: string) => void;
  openObject: (c: SavedConnection, db: string | null, schema: SchemaMeta, object: ObjectMeta) => void;
  isOpen: (key: string) => boolean;
}

function bookmarkOf({ connection, database, schema, object }: {
  connection: SavedConnection;
  database: string | null;
  schema: SchemaMeta;
  object: ObjectMeta;
}): Bookmark {
  return {
    connectionId: connection.id,
    database,
    schema: schema.name,
    currentSchema: schema.current,
    name: object.name,
    kind: object.kind,
    parent: object.parent,
  };
}

/** The navigator's built-in menu entries; features add theirs to the same scopes. */
function registerNavigatorActions(nav: RefObject<NavigatorApi>) {
  const n = () => nav.current;
  const copy = (text: string) => void navigator.clipboard.writeText(text);
  const unregister = [
    registerAction({
      id: "list.new_connection",
      scope: "navigator.list",
      title: "New connection…",
      group: "new",
      order: 10,
      run: () => workbench().newConnection(),
    }),
    registerAction({
      id: "list.test_connections",
      scope: "navigator.list",
      title: "Test connections",
      group: "test",
      order: 30,
      run: ({ project }) => testConnections(project),
    }),
    registerAction({
      id: "list.new_folder",
      scope: "navigator.list",
      title: "New folder…",
      group: "new",
      order: 11,
      when: ({ project }) => project !== null,
      run: ({ project }) => {
        if (project) newFolder(project, null);
      },
    }),
    registerAction({
      id: "list.add_existing",
      scope: "navigator.list",
      title: "Add existing…",
      group: "import",
      order: 20,
      when: ({ project }) => project !== null,
      run: () => n().props.onAddExisting(),
    }),
    registerAction({
      id: "list.import",
      scope: "navigator.list",
      title: "Import connections…",
      group: "import",
      order: 21,
      run: () => n().props.onImport(),
    }),
    registerAction({
      id: "connection.new_connection",
      scope: "navigator.connection",
      title: "New connection…",
      group: "new",
      order: 5,
      // Next to the clicked one: in its folder of the active project.
      run: ({ connection }) => workbench().newConnection(workbench().activeProject()?.placement?.[connection.id] ?? null),
    }),
    registerAction({
      id: "connection.new_script",
      scope: "navigator.connection",
      title: "New SQL script",
      group: "open",
      order: 10,
      run: ({ connection }) => n().props.onOpenTab(connection),
    }),
    registerAction({
      id: "connection.refresh",
      scope: "navigator.connection",
      title: "Refresh",
      shortcut: "F5",
      group: "refresh",
      order: 50,
      run: ({ connection }) => {
        n().refreshConnection(connection);
        n().props.onRefresh(connection);
      },
    }),
    registerAction({
      id: "connection.disconnect",
      scope: "navigator.connection",
      title: "Disconnect",
      group: "refresh",
      order: 51,
      run: ({ connection }) => n().closeConnection(connection),
    }),
    registerAction({
      id: "connection.search",
      scope: "navigator.connection",
      title: "Search objects…",
      group: "open",
      order: 20,
      run: ({ connection }) => workbench().openTab("search", connection.id, { mode: "metadata" }),
    }),
    registerAction({
      id: "connection.server_activity",
      scope: "navigator.connection",
      title: "Server activity…",
      group: "open",
      order: 25,
      // The databases whose drivers list sessions and locks (not CockroachDB nor
      // YugabyteDB, which the tab tells once connected).
      when: ({ connection }) => ["postgres", "mysql", "sqlserver"].includes(connection.driver),
      run: ({ connection }) => workbench().openTab("admin", connection.id, {}),
    }),
    registerAction({
      id: "connection.export_docs",
      scope: "navigator.connection",
      title: "Export documentation…",
      group: "open",
      order: 30,
      // The documentation covers tables and views, which MongoDB and Redis have none of.
      when: ({ connection }) => !isNoSql(connection.driver),
      run: ({ connection }) => openDocs(connection, null, null),
    }),
    registerAction({
      id: "connection.filters",
      scope: "navigator.connection",
      title: "Navigator filters…",
      group: "manage",
      order: 80,
      run: ({ connection }) =>
        workbench().openDialog((close) => <FilterDialog connection={connection} onClose={close} />),
    }),
    registerAction({
      id: "connection.copy_url",
      scope: "navigator.connection",
      title: "Copy as URL",
      group: "manage",
      order: 89,
      // Without the password; the ADBC and ODBC drivers and Oracle have no URL.
      when: ({ connection }) => hasUrl(connection.driver),
      run: async ({ connection }) => copy(await api.connectionUrl(connection.id)),
    }),
    registerAction({
      id: "connection.edit",
      scope: "navigator.connection",
      title: "Edit…",
      group: "manage",
      order: 90,
      run: ({ connection }) => n().props.onEdit(connection),
    }),
    registerAction({
      id: "connection.delete",
      scope: "navigator.connection",
      title: "Delete…",
      group: "manage",
      order: 91,
      run: ({ connection }) => n().props.onDelete(connection),
    }),
    registerAction({
      id: "database.new_script",
      scope: "navigator.database",
      title: "New SQL script",
      group: "open",
      order: 10,
      run: ({ connection, database }) => n().props.onOpenTab(connection, { database }),
    }),
    registerAction({
      id: "database.search",
      scope: "navigator.database",
      title: "Search objects…",
      group: "open",
      order: 20,
      run: ({ connection, database }) => workbench().openTab("search", connection.id, { database, mode: "metadata" }),
    }),
    registerAction({
      id: "database.refresh",
      scope: "navigator.database",
      title: "Refresh",
      shortcut: "F5",
      group: "refresh",
      order: 50,
      run: ({ connection, database, name }) => {
        const base = database === null ? connection.id : `${connection.id}@${database}`;
        const open = n().isOpen(`${connection.id}#db:${name}`) || database === null;
        // The default database's key is the connection's: its list of databases stays.
        n().refresh(base, () => n().loadSchemas(connection, database), open, true);
        n().props.onRefresh(connection, database);
      },
    }),
    registerAction({
      id: "schema.new_script",
      scope: "navigator.schema",
      title: "New SQL script",
      group: "open",
      order: 10,
      run: ({ connection, database }) => n().props.onOpenTab(connection, { database }),
    }),
    registerAction({
      id: "schema.search_data",
      scope: "navigator.schema",
      title: "Search data…",
      group: "open",
      order: 20,
      // The data search builds SQL conditions.
      when: ({ connection }) => !isNoSql(connection.driver),
      run: ({ connection, database, schema }) =>
        workbench().openTab("search", connection.id, { database, mode: "data", schema: schema.name }),
    }),
    registerAction({
      id: "schema.export_docs",
      scope: "navigator.schema",
      title: "Export documentation…",
      group: "open",
      order: 34,
      when: ({ connection }) => !isNoSql(connection.driver),
      run: ({ connection, database, schema }) => openDocs(connection, database, schema.name),
    }),
    registerAction({
      id: "schema.refresh",
      scope: "navigator.schema",
      title: "Refresh",
      shortcut: "F5",
      group: "refresh",
      order: 50,
      run: ({ connection, database, schema }) =>
        n().refresh(`${baseKey(connection, database)}/${schema.name}`, () =>
          n().loadObjects(connection, database, schema.name),
        ),
    }),
    registerAction({
      id: "object.open_data",
      scope: "navigator.object",
      title: "Open data",
      group: "open",
      order: 10,
      when: ({ object }) => isRelation(object.kind),
      run: ({ connection, database, schema, object, target }) => n().openData(connection, database, schema, object, target),
    }),
    registerAction({
      id: "object.open",
      scope: "navigator.object",
      title: "Open details",
      group: "open",
      order: 15,
      run: ({ connection, database, schema, object }) => n().openObject(connection, database, schema, object),
    }),
    registerAction({
      id: "object.copy_name",
      scope: "navigator.object",
      title: "Copy name",
      group: "copy",
      order: 40,
      multiple: true,
      // Routines are copied bare, as before: their call syntax differs from a table's.
      run: (arg) =>
        copy(
          (arg.selection ?? [arg])
            .map(({ object, target }) => (isRelation(object.kind) ? target : object.name))
            .join("\n"),
        ),
    }),
    registerAction({
      id: "object.bookmark",
      scope: "navigator.object",
      title: "Add bookmark",
      group: "copy",
      order: 45,
      when: (arg) => !isBookmarked(bookmarkOf(arg)),
      run: (arg) => toggleBookmark(bookmarkOf(arg)),
    }),
    registerAction({
      id: "object.unbookmark",
      scope: "navigator.object",
      title: "Remove bookmark",
      group: "copy",
      order: 45,
      when: (arg) => isBookmarked(bookmarkOf(arg)),
      run: (arg) => toggleBookmark(bookmarkOf(arg)),
    }),
    registerAction({
      id: "object.refresh",
      scope: "navigator.object",
      title: "Refresh",
      shortcut: "F5",
      group: "refresh",
      order: 60,
      // A relation reloads its details, another object the objects of its schema.
      run: ({ connection, database, schema, object }) => {
        const key = `${baseKey(connection, database)}/${schema.name}`;
        if (isRelation(object.kind)) {
          n().refresh(`${key}/${object.name}`, () => void n().loadDetails(connection, database, schema.name, object.name));
        } else {
          n().refresh(key, () => n().loadObjects(connection, database, schema.name));
        }
      },
    }),
    registerAction({
      id: "column.copy_name",
      scope: "navigator.column",
      title: "Copy name",
      group: "copy",
      order: 40,
      run: ({ column }) => copy(column),
    }),
  ];
  return () => unregister.forEach((u) => u());
}
