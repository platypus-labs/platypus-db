import { type RefObject, useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  type Capabilities,
  toCommandError,
  type ObjectKind,
  type ObjectMeta,
  type SavedConnection,
  type SchemaMeta,
  type TableDetails,
} from "../api";
import { onNavigatorRefresh } from "../navigatorEvents";
import { qualifiedName, viewerSql } from "../sqlNames";
import type { TabInit } from "../tabs/registry";
import { type ThemePref, applyTheme, loadTheme, nextTheme } from "../theme";
import { registerAction } from "../actions";
import { workbench } from "../workbench";
import { ContextMenu, type MenuState, type MenuTarget } from "./ContextMenu";

type Load<T> = { state: "loading" } | { state: "error"; message: string } | { state: "ok"; data: T };

/**
 * Loaded metadata by node key. A database's base key is the connection id for the saved
 * database and `cid@name` for the others; below it `base` → schemas, `base/schema` →
 * objects, `base/schema/name` → details. `cid#dbs` holds the server's databases.
 */
type Cache = Map<string, Load<unknown>>;

interface Props {
  connections: SavedConnection[];
  error: string | null;
  onNew: () => void;
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
  /** A connection was refreshed: cached metadata elsewhere (autocompletion) is stale. */
  onRefresh: (c: SavedConnection, database?: string | null) => void;
  onError: (message: string) => void;
}

const FOLDERS: { kind: ObjectKind; label: string; icon: string }[] = [
  { kind: "table", label: "Tables", icon: "▦" },
  { kind: "view", label: "Views", icon: "◫" },
  { kind: "materialized_view", label: "Materialized views", icon: "◩" },
  { kind: "foreign_table", label: "Foreign tables", icon: "⧉" },
  { kind: "sequence", label: "Sequences", icon: "#" },
  { kind: "function", label: "Functions", icon: "ƒ" },
  { kind: "procedure", label: "Procedures", icon: "ƒ" },
  { kind: "trigger", label: "Triggers", icon: "⚡" },
  { kind: "type", label: "Types", icon: "τ" },
  { kind: "domain", label: "Domains", icon: "τ" },
  { kind: "extension", label: "Extensions", icon: "⬡" },
  { kind: "event", label: "Events", icon: "⏱" },
];

export const OBJECT_ICONS = Object.fromEntries(FOLDERS.map((f) => [f.kind, f.icon])) as Record<ObjectKind, string>;

export const isRelation = (kind: ObjectKind) =>
  kind === "table" || kind === "view" || kind === "materialized_view" || kind === "foreign_table";

/** The cache key under which a database's schemas live. */
const baseKey = (c: SavedConnection, database: string | null) => (database === null ? c.id : `${c.id}@${database}`);

/** `key` itself or a node below it. */
const isUnder = (k: string, key: string) =>
  k === key || k.startsWith(`${key}/`) || k.startsWith(`${key}@`) || k.startsWith(`${key}#`);

interface Row {
  key: string;
  depth: number;
  label: string;
  icon?: string;
  color?: string | null;
  detail?: string;
  title?: string;
  muted?: boolean;
  /** `undefined`: leaf. */
  expanded?: boolean;
  onToggle?: () => void;
  onOpen?: () => void;
  /** Its context menu lists the actions registered for this scope. */
  menu?: MenuTarget;
  actions?: React.ReactNode;
  status?: "loading" | "error";
}

export function Navigator(props: Props) {
  const { connections, error, onNew, onEdit, onOpenTab, session, disconnect, onError } = props;
  const [cache, setCache] = useState<Cache>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState<MenuState | null>(null);
  const signatures = useRef(new Map<string, string>());

  // Forget what was loaded through a connection whose target changed.
  useEffect(() => {
    for (const c of connections) {
      const signature = JSON.stringify([c.driver, c.host, c.port, c.database, c.user, c.ssl_mode]);
      const previous = signatures.current.get(c.id);
      signatures.current.set(c.id, signature);
      if (previous !== undefined && previous !== signature) closeConnection(c);
    }
  }, [connections]);

  const setLoad = (key: string, value: Load<unknown>) => setCache((m) => new Map(m).set(key, value));
  const loaded = <T,>(key: string) => {
    const entry = cache.get(key);
    return entry?.state === "ok" ? (entry.data as T) : null;
  };

  async function load<T>(c: SavedConnection, db: string | null, key: string, fetch: (sid: number) => Promise<T>) {
    setLoad(key, { state: "loading" });
    const sid = await session(c, db);
    if (sid === null) {
      setCache((m) => {
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
    if (open && !cache.has(key)) onOpen?.();
  }

  /** Forgets everything loaded under `key` and reloads it if open (`open`: whether the
   *  node showing it is expanded, when that node has another key). */
  function refresh(key: string, reload: () => void, open = expanded.has(key)) {
    setCache((m) => new Map([...m].filter(([k]) => !isUnder(k, key))));
    if (open) reload();
  }

  function refreshConnection(c: SavedConnection) {
    refresh(c.id, () => {
      loadSchemas(c, null);
      void loadDatabases(c);
    });
  }

  function closeConnection(c: SavedConnection) {
    setCache((m) => new Map([...m].filter(([k]) => !isUnder(k, c.id))));
    setExpanded((s) => withoutPrefix(s, c.id, true));
    disconnect(c);
  }

  function openData(c: SavedConnection, db: string | null, schema: SchemaMeta, object: ObjectMeta, target: string) {
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

  for (const c of connections) {
    const ckey = c.id;
    const open = expanded.has(ckey);
    rows.push({
      key: ckey,
      depth: 0,
      label: c.name,
      color: c.color,
      title: c.driver === "sqlite" ? c.database : `${c.user}@${c.host}:${c.port ?? ""}/${c.database}`,
      expanded: open,
      onToggle: () =>
        toggle(ckey, () => {
          loadSchemas(c, null);
          void loadDatabases(c);
        }),
      onOpen: () => onOpenTab(c),
      ...status(ckey),
      menu: { scope: "navigator.connection", arg: { connection: c } },
      actions: (
        <>
          <button className="icon" title="Open SQL editor" onClick={() => onOpenTab(c)}>
            ▶
          </button>
          <button className="icon" title="Edit" onClick={() => onEdit(c)}>
            ✎
          </button>
        </>
      ),
    });
    if (!open) continue;
    const databases = loaded<string[]>(`${c.id}#dbs`);
    if (!databases || databases.length < 2) {
      rows.push(...schemaRows(c, null, 1));
      continue;
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
        icon: "⛁",
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

  function schemaRows(c: SavedConnection, db: string | null, depth: number): Row[] {
    const out: Row[] = [];
    const base = baseKey(c, db);
    const schemas = loaded<SchemaMeta[]>(base);
    if (!schemas) {
      const s = status(base);
      if (s.status) out.push({ key: `${base}#status`, depth, label: s.detail ?? "", muted: true, ...s });
      return out;
    }
    // User schemas first, catalog schemas after them.
    const ordered = [...schemas].sort((a, b) => Number(a.system) - Number(b.system));
    for (const schema of ordered) {
      const skey = `${base}/${schema.name}`;
      const objects = loaded<ObjectMeta[]>(skey);
      const matches = objects?.filter((o) => !needle || o.name.toLowerCase().includes(needle));
      if (needle && objects && matches?.length === 0) continue;
      const sOpen = expanded.has(skey) || (!!needle && !!matches?.length);
      out.push({
        key: skey,
        depth,
        label: schema.name,
        icon: "◇",
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
      for (const folder of FOLDERS) {
        const items = matches.filter((o) => o.kind === folder.kind);
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
          out.push(...objectRows(c, db, schema, object, folder.icon, depth + 2));
        }
      }
    }
    return out;
  }

  function objectRows(
    c: SavedConnection,
    db: string | null,
    schema: SchemaMeta,
    object: ObjectMeta,
    icon: string,
    depth: number,
  ): Row[] {
    // Triggers are named per table in PostgreSQL: the key includes the table.
    const okey = `${baseKey(c, db)}/${schema.name}/${object.parent ? `${object.parent}.` : ""}${object.name}`;
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
      if (count === 0) return;
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
        icon: col.primary_key ? "⚿" : "·",
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
        icon: ix.primary ? "⚿" : "↯",
        detail: `(${ix.columns.join(", ")})${ix.primary ? " · primary" : ix.unique ? " · unique" : ""}`,
      })),
    );
    section(
      "Foreign keys",
      details.foreign_keys.length,
      details.foreign_keys.map((fk) => ({
        key: `${okey}:fk:${fk.name}`,
        depth: depth + 2,
        label: fk.name,
        icon: "→",
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
        icon: "←",
        detail: `(${r.columns.join(", ")}) → (${r.ref_columns.join(", ")})`,
        title: r.name,
      })),
    );
    return out;
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">Connections</span>
        <ThemeButton />
        <button className="icon" title="Settings (Ctrl+,)" onClick={() => workbench().openSettings()}>
          ⚙
        </button>
        <button className="icon" title="New connection" onClick={onNew}>
          +
        </button>
      </div>
      {connections.length > 0 && (
        <input
          className="tree-filter"
          placeholder="Filter objects…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setFilter("")}
        />
      )}
      {error && <div className="notice error small">{error}</div>}
      {connections.length === 0 && !error && (
        <button className="empty-cta" onClick={onNew}>
          Add your first connection
        </button>
      )}
      <ul className="tree" role="tree">
        {rows.map((row) => (
          <li
            key={row.key}
            role="treeitem"
            aria-expanded={row.expanded}
            className={`tree-row${row.muted ? " muted" : ""}${row.status === "error" ? " failed" : ""}`}
            style={{ paddingLeft: 6 + row.depth * 14 }}
            title={
              row.status === "error"
                ? row.detail
                : (row.title ?? (row.detail ? `${row.label}  ${row.detail}` : undefined))
            }
            onClick={() => row.onToggle?.()}
            onDoubleClick={(e) => {
              e.preventDefault();
              row.onOpen?.();
            }}
            onContextMenu={(e) => {
              if (!row.menu) return;
              e.preventDefault();
              setMenu({ x: e.clientX, y: e.clientY, target: row.menu });
            }}
          >
            <span className="twisty">{row.expanded === undefined ? "" : row.expanded ? "▾" : "▸"}</span>
            {row.color !== undefined ? (
              <span className="dot" style={{ background: row.color ?? "var(--border)" }} />
            ) : (
              row.icon && <span className="tree-icon">{row.icon}</span>
            )}
            <span className="tree-label">{row.label}</span>
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

const THEME_LABELS: Record<ThemePref, { icon: string; label: string }> = {
  system: { icon: "◐", label: "System" },
  light: { icon: "☀", label: "Light" },
  dark: { icon: "☾", label: "Dark" },
};

function ThemeButton() {
  const [pref, setPref] = useState(loadTheme);
  const { icon, label } = THEME_LABELS[pref];
  return (
    <button
      className="icon"
      title={`Theme: ${label} (click to change)`}
      onClick={() => {
        const next = nextTheme[pref];
        applyTheme(next);
        setPref(next);
      }}
    >
      {icon}
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
      // Routines are copied bare, as before: their call syntax differs from a table's.
      run: ({ object, target }) => copy(isRelation(object.kind) ? target : object.name),
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
