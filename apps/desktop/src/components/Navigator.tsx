import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type Capabilities,
  isEmbedded,
  toCommandError,
  type ObjectKind,
  type ObjectMeta,
  type Project,
  type SavedConnection,
  type SchemaMeta,
  type TableDetails,
} from "../api";
import { defineSetting, useSetting } from "../settings";
import { dropCached, loadCached, pruneCached, saveCached } from "../navigator/diskCache";
import { FilterDialog } from "../navigator/FilterDialog";
import { type Bookmark, isBookmarked, nameMatcher, toggleBookmark, useNavigatorPrefs } from "../navigator/prefs";
import { onNavigatorRefresh } from "../navigatorEvents";
import { toggled } from "../sets";
import { qualifiedName, viewerSql } from "../sqlNames";
import type { TabInit, TabModel } from "../tabs/registry";
import { PRESETS, cycleTheme, themeName, themeSetting } from "../theme";
import { actionsFor, registerAction, runAction, shortcutLabel } from "../actions";
import { workbench } from "../workbench";
import { connectionColor, typeBadge } from "../connections";
import { Icon, type IconName, objectIcon } from "../ui/Icon";
import { ContextMenu, type MenuState, type MenuTarget } from "./ContextMenu";
import { type FolderNode, folderTree, newFolder } from "../projects/folders";

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

function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${unit === 0 ? value : value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** `~1.2K rows · 3.4 MB`, from what the statistics say. */
function sizeDetail(object: ObjectMeta): string | undefined {
  const parts = [object.rows != null && `~${compact.format(object.rows)} rows`, object.size != null && formatBytes(object.size)];
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

const FOLDERS: { kind: ObjectKind; label: string }[] = [
  { kind: "table", label: "Tables" },
  { kind: "view", label: "Views" },
  { kind: "materialized_view", label: "Materialized views" },
  { kind: "foreign_table", label: "Foreign tables" },
  { kind: "sequence", label: "Sequences" },
  { kind: "function", label: "Functions" },
  { kind: "procedure", label: "Procedures" },
  { kind: "trigger", label: "Triggers" },
  { kind: "type", label: "Types" },
  { kind: "domain", label: "Domains" },
  { kind: "extension", label: "Extensions" },
  { kind: "event", label: "Events" },
  { kind: "dictionary", label: "Dictionaries" },
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
  title?: string;
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
  /** A connection or folder that can be dragged into a folder of the project. */
  drag?: DragItem;
  /** Takes a dragged connection or folder. */
  onDrop?: (item: DragItem) => void;
}

interface DragItem {
  kind: "connection" | "folder";
  id: string;
}

const DRAG_TYPE = "application/x-platypus-navigator";

export function Navigator(props: Props) {
  const { connections, project, onAddExisting, error, onNew, onImport, onEdit, onOpenTab, session, disconnect, active, onError } = props;
  // The ref is the source of truth, so a load sees what was merged just before it.
  const cacheRef = useRef<Cache>(new Map());
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
  const prefs = useNavigatorPrefs();
  const simple = useSetting(viewSetting) === "simple";
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  /** The selected rows; `anchor` is where a Shift+click range starts. */
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const anchor = useRef<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const signatures = useRef(new Map<string, string>());

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
    const sid = await session(c, db);
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
      setLoad(key, { state: "ok", data });
      setOffline(c.id, false);
      scheduleSave(c.id);
      return data;
    } catch (e) {
      setLoad(key, { state: "error", message: toCommandError(e).message });
      return null;
    }
  }

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
    if (open && (!cache.has(key) || isCached(key))) onOpen?.();
  }

  /** Forgets everything loaded under `key` and reloads it if open (`open`: whether the
   *  node showing it is expanded, when that node has another key). */
  function refresh(key: string, reload: () => void, open = expanded.has(key)) {
    updateCache((m) => new Map([...m].filter(([k]) => !isUnder(k, key))));
    if (open) reload();
  }

  async function openConnection(c: SavedConnection) {
    await restoreCache(c);
    loadSchemas(c, null);
    void loadDatabases(c);
  }

  function refreshConnection(c: SavedConnection) {
    setOffline(c.id, false);
    refresh(c.id, () => {
      loadSchemas(c, null);
      void loadDatabases(c);
    });
  }

  function closeConnection(c: SavedConnection) {
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
      sql: viewerSql(c.driver, target, "", null),
      autorun: true,
      title: object.name,
      table: target,
      tableRef: { schema: schema.name, name: object.name, kind: object.kind },
      database: db,
    });
  }

  function openObject(c: SavedConnection, db: string | null, schema: SchemaMeta, object: ObjectMeta) {
    const showing = (t: TabModel) =>
      t.kind === "object" &&
      t.connectionId === c.id &&
      (t.database ?? null) === db &&
      t.schema === schema.name &&
      t.name === object.name &&
      t.objectKind === object.kind &&
      (t.parent ?? null) === (object.parent ?? null);
    if (workbench().focusTab(showing)) return;
    workbench().openTab("object", c.id, {
      database: db,
      schema: schema.name,
      currentSchema: schema.current,
      name: object.name,
      objectKind: object.kind,
      parent: object.parent,
      title: object.name,
    });
  }

  // What the navigator's own actions need, refreshed every render.
  const nav = useRef<NavigatorApi>(null!);
  nav.current = {
    props,
    refresh,
    refreshConnection,
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
        if (schema === undefined) n.refresh(base, () => n.loadSchemas(c, db), open);
        else n.refresh(`${base}/${schema}`, () => n.loadObjects(c, db, schema));
      }),
    [],
  );

  const needle = filter.trim().toLowerCase();
  const rows: Row[] = [];
  const status = (key: string): Pick<Row, "status" | "detail"> => {
    const entry = cache.get(key);
    if (entry?.state === "loading") return { status: "loading", detail: "loading…" };
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
      detail: [isOffline && "offline", filtered && "filtered"].filter(Boolean).join(" · ") || undefined,
      title: isOffline
        ? `${target}\nNot connected: showing the metadata cached on disk. Refresh to retry.`
        : `${target}${active.has(c.id) ? "\nConnected" : ""}`,
      expanded: open,
      connected: active.has(c.id),
      onToggle: () => toggle(ckey, () => void openConnection(c)),
      onOpen: () => onOpenTab(c),
      ...status(ckey),
      menu: { scope: "navigator.connection", arg: { connection: c } },
      drag: project ? { kind: "connection", id: c.id } : undefined,
      actions: (
        <>
          <button className="icon" title="Open SQL editor" onClick={() => onOpenTab(c)}>
            <Icon name="sql" />
          </button>
          <button className="icon" title="Edit" onClick={() => onEdit(c)}>
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
        detail: db === null ? "default" : undefined,
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

  /** Drops a dragged connection or folder into `folder`, or at the root for `null`. */
  function dropInto(p: Project, folder: string | null, item: DragItem) {
    if (item.id === folder) return;
    const change =
      item.kind === "connection"
        ? api.placeConnection(p.id, item.id, folder)
        : api.moveConnectionFolder(p.id, item.id, folder);
    change.then(workbench().setProjects, (e: unknown) => onError(toCommandError(e).message));
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
      drag: { kind: "folder", id: node.folder.id },
      onDrop: (item) => dropInto(p, node.folder.id, item),
    });
    if (!fOpen) return;
    for (const child of node.children) addFolder(p, child, level + 1);
    for (const c of node.connections) addConnection(c, level + 1);
  }

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
      if (s.status) out.push({ key: `${base}#status`, depth, label: s.detail ?? "", muted: true, ...s });
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
          (!simple || SIMPLE_KINDS.has(o.kind)) &&
          !hiddenObject(o.name),
      );
      if (needle && objects && matches?.length === 0) continue;
      const sOpen = expanded.has(skey) || (!!needle && !!matches?.length);
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
        out.push({ key: `${skey}#empty`, depth: depth + 1, label: "No objects", muted: true });
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
          expanded.has(fkey) || !!needle || (folder.kind === "table" && !expanded.has(`${fkey}!`));
        out.push({
          key: fkey,
          depth: depth + 1,
          label: folder.label,
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
        label: "Bookmarks",
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
    // Triggers are named per table in PostgreSQL: the key includes the table.
    const okey = `${baseKey(c, db)}/${schema.name}/${object.kind === "trigger" && object.parent ? `${object.parent}.` : ""}${object.name}`;
    const target = qualifiedName(c.driver, schema, object.name);
    const menu: MenuTarget = { scope: "navigator.object", arg: { connection: c, database: db, schema, object, target } };
    if (!isRelation(object.kind)) {
      return [
        {
          key: okey,
          depth,
          label: object.name,
          icon,
          detail: object.parent ? `on ${object.parent}` : undefined,
          title: object.comment ?? undefined,
          onOpen: () => openObject(c, db, schema, object),
          menu,
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
        detail: sizeDetail(object),
        title: object.comment ?? undefined,
        expanded: open,
        onToggle: () => toggle(okey, () => void loadDetails(c, db, schema.name, object.name)),
        onOpen: () => openData(c, db, schema, object, target),
        ...status(okey),
        menu,
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
        label: name,
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
      "Columns",
      details.columns.length,
      details.columns.map((col) => ({
        key: `${okey}.${col.name}`,
        depth: depth + 2,
        label: col.name,
        icon: col.primary_key ? "key" : "column",
        detail: `${col.data_type}${col.nullable ? "" : " · not null"}`,
        title: [col.default && `default ${col.default}`, col.comment].filter(Boolean).join("\n") || undefined,
        menu: {
          scope: "navigator.column",
          arg: { connection: c, database: db, schema, object, column: col.name },
        },
      })),
    );
    section(
      "Indexes",
      details.indexes.length,
      details.indexes.map((ix) => ({
        key: `${okey}:ix:${ix.name}`,
        depth: depth + 2,
        label: ix.name,
        icon: ix.primary ? "key" : "index",
        detail: `(${ix.columns.join(", ")})${ix.primary ? " · primary" : ix.unique ? " · unique" : ""}${ix.size != null ? ` · ${formatBytes(ix.size)}` : ""}`,
      })),
    );
    const parts = partitions.get(object.name) ?? [];
    section(
      "Partitions",
      parts.length,
      parts.flatMap((p) => objectRows(c, db, schema, p, partitions, depth + 2)),
    );
    section(
      "Foreign keys",
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
      "References",
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
            "Connections"
          )}
        </span>
        <ThemeButton />
        <button className="icon" title="Settings (Ctrl+,)" onClick={() => workbench().openSettings()}>
          <Icon name="settings" />
        </button>
        <button className="icon" title="Import connections from DBeaver, DataGrip, pgAdmin…" onClick={onImport}>
          <Icon name="import" />
        </button>
        {project && (
          <button className="icon" title="New folder" onClick={() => newFolder(project, null)}>
            <Icon name="new-folder" />
          </button>
        )}
        <button className="icon" title="New connection" onClick={onNew}>
          <Icon name="plus" />
        </button>
      </div>
      {connections.length > 0 && (
        <div className="tree-search">
          <Icon name="search" className="tree-search-icon" />
          <input
            className="tree-filter"
            placeholder="Filter objects…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setFilter("")}
          />
          <PaletteHint />
        </div>
      )}
      {error && <div className="notice error small">{error}</div>}
      {project && connections.length === 0 && !error && (
        <div className="project-empty">
          <p className="muted small">No connections in this project.</p>
          <button onClick={onNew}>New connection</button>
          <button onClick={onAddExisting}>Add existing…</button>
          <button onClick={onImport}>Import…</button>
        </div>
      )}
      {!project && connections.length === 0 && !error && (
        <>
          <button className="empty-cta" onClick={onNew}>
            Add your first connection
          </button>
          <button className="link small empty-import" onClick={onImport}>
            or import them from DBeaver, DataGrip, pgAdmin…
          </button>
        </>
      )}
      <ul
        className="tree"
        role="tree"
        onDragOver={(e) => project && e.dataTransfer.types.includes(DRAG_TYPE) && e.preventDefault()}
        onDrop={(e) => {
          // Dropped below the rows: to the project's root.
          const data = e.dataTransfer.getData(DRAG_TYPE);
          if (!project || !data) return;
          e.preventDefault();
          dropInto(project, null, JSON.parse(data) as DragItem);
        }}
      >
        {rows.map((row) => (
          <li
            key={row.key}
            role="treeitem"
            aria-expanded={row.expanded}
            className={`tree-row${row.muted ? " muted" : ""}${row.status === "error" ? " failed" : ""}${selected.has(row.key) ? " selected" : ""}`}
            style={{ paddingLeft: 6 + row.depth * 16 }}
            draggable={!!row.drag}
            onDragStart={(e) => row.drag && e.dataTransfer.setData(DRAG_TYPE, JSON.stringify(row.drag))}
            onDragOver={(e) => {
              if (!row.onDrop || !e.dataTransfer.types.includes(DRAG_TYPE)) return;
              e.preventDefault();
              e.stopPropagation();
            }}
            onDrop={(e) => {
              const data = e.dataTransfer.getData(DRAG_TYPE);
              if (!row.onDrop || !data) return;
              e.preventDefault();
              e.stopPropagation();
              row.onDrop(JSON.parse(data) as DragItem);
            }}
            title={
              row.status === "error"
                ? row.detail
                : (row.title ?? (row.detail ? `${row.label}  ${row.detail}` : undefined))
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
            {row.connected && <span className="connected-mark" role="img" aria-label="Connected" />}
            {row.badge && <span className={`type-badge ${row.badge.toLowerCase()}`}>{row.badge}</span>}
            {row.detail && <span className="tree-detail">{row.detail}</span>}
            {row.actions && (
              <span className="tree-actions" onClick={(e) => e.stopPropagation()}>
                {row.actions}
              </span>
            )}
          </li>
        ))}
      </ul>
      {menu && (
        <ContextMenu menu={menu} onClose={closeMenu} onError={(e) => onError(toCommandError(e).message)} />
      )}
    </aside>
  );
}

/** The palette's shortcut beside the filter; a click opens the palette. */
function PaletteHint() {
  const action = actionsFor("global", undefined).find((a) => a.id === "app.palette");
  const keys = action && shortcutLabel(action);
  if (!action || !keys) return null;
  return (
    <kbd className="tree-search-kbd" title="Search everything" onClick={() => runAction(action, undefined, () => {})}>
      {keys}
    </kbd>
  );
}

/** Cycles system, Platypus Light and Platypus Dark; the other themes are in the Settings. */
function ThemeButton() {
  useSetting(themeSetting);
  const name = themeName();
  const label = name === "system" ? "System" : PRESETS[name].label;
  return (
    <button className="icon" title={`Theme: ${label} (click to change; more in the Settings)`} onClick={cycleTheme}>
      <Icon name="theme" />
    </button>
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
  refresh: (key: string, reload: () => void, open?: boolean) => void;
  refreshConnection: (c: SavedConnection) => void;
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
      id: "connection.filters",
      scope: "navigator.connection",
      title: "Navigator filters…",
      group: "manage",
      order: 80,
      run: ({ connection }) =>
        workbench().openDialog((close) => <FilterDialog connection={connection} onClose={close} />),
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
      group: "refresh",
      order: 50,
      run: ({ connection, database, name }) => {
        const base = database === null ? connection.id : `${connection.id}@${database}`;
        const open = n().isOpen(`${connection.id}#db:${name}`) || database === null;
        n().refresh(base, () => n().loadSchemas(connection, database), open);
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
      when: ({ connection }) => connection.driver !== "mongo",
      run: ({ connection, database, schema }) =>
        workbench().openTab("search", connection.id, { database, mode: "data", schema: schema.name }),
    }),
    registerAction({
      id: "schema.refresh",
      scope: "navigator.schema",
      title: "Refresh",
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
      group: "refresh",
      order: 60,
      when: ({ object }) => isRelation(object.kind),
      run: ({ connection, database, schema, object }) =>
        n().refresh(`${baseKey(connection, database)}/${schema.name}/${object.name}`, () => {
          void n().loadDetails(connection, database, schema.name, object.name);
        }),
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
