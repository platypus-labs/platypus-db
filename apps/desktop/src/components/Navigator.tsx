import { useEffect, useRef, useState } from "react";
import {
  api,
  toCommandError,
  type ObjectKind,
  type ObjectMeta,
  type SavedConnection,
  type SchemaMeta,
  type TableDetails,
} from "../api";
import { OPEN_DATA_LIMIT, qualifiedName, selectSql, viewerSql } from "../sqlNames";
import type { TabModel } from "./QueryTab";
import { type ThemePref, applyTheme, loadTheme, nextTheme } from "../theme";

type Load<T> = { state: "loading" } | { state: "error"; message: string } | { state: "ok"; data: T };

/** Loaded metadata by node key: `cid` → schemas, `cid/schema` → objects, `cid/schema/name` → details. */
type Cache = Map<string, Load<unknown>>;

interface Props {
  connections: SavedConnection[];
  error: string | null;
  onNew: () => void;
  onEdit: (c: SavedConnection) => void;
  onDelete: (c: SavedConnection) => void;
  /** Opens a new editor tab: optionally with SQL, run at once, or as a data viewer. */
  onOpenTab: (c: SavedConnection, init?: Partial<TabModel>) => void;
  /** The connection's navigator session, opened on first use; `null` if cancelled. */
  session: (c: SavedConnection) => Promise<number | null>;
  /** Drops the navigator session, e.g. after the connection broke. */
  disconnect: (c: SavedConnection) => void;
  /** A connection was refreshed: cached metadata elsewhere (autocompletion) is stale. */
  onRefresh: (c: SavedConnection) => void;
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
];

const isRelation = (kind: ObjectKind) =>
  kind === "table" || kind === "view" || kind === "materialized_view" || kind === "foreign_table";

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
  menu?: MenuItem[];
  actions?: React.ReactNode;
  status?: "loading" | "error";
}

interface MenuItem {
  label: string;
  run: () => void;
}

export function Navigator(props: Props) {
  const { connections, error, onNew, onEdit, onDelete, onOpenTab, session, disconnect, onRefresh, onError } =
    props;
  const [cache, setCache] = useState<Cache>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
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

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  const setLoad = (key: string, value: Load<unknown>) => setCache((m) => new Map(m).set(key, value));
  const loaded = <T,>(key: string) => {
    const entry = cache.get(key);
    return entry?.state === "ok" ? (entry.data as T) : null;
  };

  async function load<T>(c: SavedConnection, key: string, fetch: (sid: number) => Promise<T>) {
    setLoad(key, { state: "loading" });
    const sid = await session(c);
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

  function loadSchemas(c: SavedConnection) {
    void load(c, c.id, (sid) => api.listSchemas(sid)).then((schemas) => {
      // Open the current schema straight away: it is almost always the one wanted.
      const current = schemas?.find((s) => s.current);
      if (current) {
        const key = `${c.id}/${current.name}`;
        setExpanded((s) => new Set(s).add(key));
        loadObjects(c, current.name);
      }
    });
  }

  const loadObjects = (c: SavedConnection, schema: string) =>
    void load(c, `${c.id}/${schema}`, (sid) => api.listObjects(sid, schema));

  const loadDetails = (c: SavedConnection, schema: string, name: string) =>
    load(c, `${c.id}/${schema}/${name}`, (sid) => api.tableDetails(sid, schema, name));

  function toggle(key: string, onOpen?: () => void) {
    const open = !expanded.has(key);
    // Collapsing keeps the state below, so reopening shows the same nodes.
    setExpanded((s) => (open ? new Set(s).add(key) : withoutPrefix(s, key)));
    if (open && !cache.has(key)) onOpen?.();
  }

  /** Forgets everything loaded under `key` and reloads it if open. */
  function refresh(key: string, reload: () => void) {
    setCache((m) => new Map([...m].filter(([k]) => k !== key && !k.startsWith(`${key}/`))));
    if (expanded.has(key)) reload();
  }

  function closeConnection(c: SavedConnection) {
    setCache((m) => new Map([...m].filter(([k]) => k !== c.id && !k.startsWith(`${c.id}/`))));
    setExpanded((s) => withoutPrefix(s, c.id, true));
    disconnect(c);
  }

  async function generateSelect(c: SavedConnection, schema: SchemaMeta, object: ObjectMeta) {
    const key = `${c.id}/${schema.name}/${object.name}`;
    const details = loaded<TableDetails>(key) ?? (await loadDetails(c, schema.name, object.name));
    if (!details) return;
    const target = qualifiedName(c.driver, schema, object.name);
    onOpenTab(c, {
      sql: selectSql(
        c.driver,
        target,
        details.columns.map((col) => col.name),
        OPEN_DATA_LIMIT,
      ),
      title: object.name,
    });
  }

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
      onToggle: () => toggle(ckey, () => loadSchemas(c)),
      onOpen: () => onOpenTab(c),
      ...status(ckey),
      menu: [
        { label: "New SQL script", run: () => onOpenTab(c) },
        {
          label: "Refresh",
          run: () => {
            refresh(ckey, () => loadSchemas(c));
            onRefresh(c);
          },
        },
        { label: "Disconnect", run: () => closeConnection(c) },
        { label: "Edit…", run: () => onEdit(c) },
        { label: "Delete…", run: () => onDelete(c) },
      ],
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
    const schemas = loaded<SchemaMeta[]>(ckey);
    if (!schemas) continue;
    // User schemas first, catalog schemas after them.
    const ordered = [...schemas].sort((a, b) => Number(a.system) - Number(b.system));
    for (const schema of ordered) {
      const skey = `${ckey}/${schema.name}`;
      const objects = loaded<ObjectMeta[]>(skey);
      const matches = objects?.filter((o) => !needle || o.name.toLowerCase().includes(needle));
      if (needle && objects && matches?.length === 0) continue;
      const sOpen = expanded.has(skey) || (!!needle && !!matches?.length);
      rows.push({
        key: skey,
        depth: 1,
        label: schema.name,
        icon: "◇",
        muted: schema.system,
        expanded: sOpen,
        onToggle: () => toggle(skey, () => loadObjects(c, schema.name)),
        ...status(skey),
        menu: [
          { label: "New SQL script", run: () => onOpenTab(c) },
          { label: "Refresh", run: () => refresh(skey, () => loadObjects(c, schema.name)) },
        ],
      });
      if (!sOpen || !matches) continue;
      if (matches.length === 0) {
        rows.push({ key: `${skey}#empty`, depth: 2, label: "No objects", muted: true });
        continue;
      }
      for (const folder of FOLDERS) {
        const items = matches.filter((o) => o.kind === folder.kind);
        if (items.length === 0) continue;
        const fkey = `${skey}#${folder.kind}`;
        const fOpen =
          expanded.has(fkey) || !!needle || (folder.kind === "table" && !expanded.has(`${fkey}!`));
        rows.push({
          key: fkey,
          depth: 2,
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
          rows.push(...objectRows(c, schema, object, folder.icon));
        }
      }
    }
  }

  function objectRows(c: SavedConnection, schema: SchemaMeta, object: ObjectMeta, icon: string): Row[] {
    const okey = `${c.id}/${schema.name}/${object.name}`;
    const target = qualifiedName(c.driver, schema, object.name);
    if (!isRelation(object.kind)) {
      return [
        {
          key: okey,
          depth: 3,
          label: object.name,
          icon,
          title: object.comment ?? undefined,
          menu: [{ label: "Copy name", run: () => void navigator.clipboard.writeText(object.name) }],
        },
      ];
    }
    const openData = () =>
      onOpenTab(c, {
        sql: viewerSql(c.driver, target, "", null),
        autorun: true,
        title: object.name,
        table: target,
      });
    const open = expanded.has(okey);
    const out: Row[] = [
      {
        key: okey,
        depth: 3,
        label: object.name,
        icon,
        title: object.comment ?? undefined,
        expanded: open,
        onToggle: () => toggle(okey, () => void loadDetails(c, schema.name, object.name)),
        onOpen: openData,
        ...status(okey),
        menu: [
          { label: "Open data", run: openData },
          { label: "Generate SELECT", run: () => void generateSelect(c, schema, object) },
          { label: "Copy name", run: () => void navigator.clipboard.writeText(target) },
          { label: "Refresh", run: () => refresh(okey, () => void loadDetails(c, schema.name, object.name)) },
        ],
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
        depth: 4,
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
        depth: 5,
        label: col.name,
        icon: col.primary_key ? "⚿" : "·",
        detail: `${col.data_type}${col.nullable ? "" : " · not null"}`,
        title: [col.default && `default ${col.default}`, col.comment].filter(Boolean).join("\n") || undefined,
        menu: [{ label: "Copy name", run: () => void navigator.clipboard.writeText(col.name) }],
      })),
    );
    section(
      "Indexes",
      details.indexes.length,
      details.indexes.map((ix) => ({
        key: `${okey}:ix:${ix.name}`,
        depth: 5,
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
        depth: 5,
        label: fk.name,
        icon: "→",
        detail: `(${fk.columns.join(", ")}) → ${fk.ref_table}(${fk.ref_columns.join(", ")})`,
      })),
    );
    return out;
  }

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">Connections</span>
        <ThemeButton />
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
              setMenu({ x: e.clientX, y: e.clientY, items: row.menu });
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
        <div
          className="context-menu"
          style={{ left: menu.x, top: menu.y }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {menu.items.map((item) => (
            <button
              key={item.label}
              onClick={() => {
                setMenu(null);
                try {
                  item.run();
                } catch (e) {
                  onError(toCommandError(e).message);
                }
              }}
            >
              {item.label}
            </button>
          ))}
        </div>
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
    [...set].filter((k) => k !== key && !(children && (k.startsWith(`${key}/`) || k.startsWith(`${key}#`)))),
  );
}
