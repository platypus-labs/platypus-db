import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { actionsFor, runAction, shortcutLabel } from "../actions";
import { api, type CompletionCatalog, type HistoryEntry, isEmbedded, type SavedConnection, type ScriptList, toCommandError } from "../api";
import { connectionFor, nameOf, openScript } from "../scripts/scripts";
import { projectKey } from "../projects/state";
import { qualifiedName, viewerSql } from "../sqlNames";
import { parseDbKey, workbench } from "../workbench";
import { Icon, type IconName } from "../ui/Icon";
import { type FuzzyMatch, fuzzy, highlightRuns } from "./fuzzy";
import { t } from "../i18n";

/** The icon of each kind of result, by the kind's untranslated name. */
const KIND_ICONS: Record<string, IconName> = { Action: "run", Project: "folder", Connection: "database", "Table or view": "table", "Recent query": "history", Script: "sql" };

/** Results shown at most. */
const LIMIT = 60;

interface Item {
  key: string;
  /** What it is, as shown: Action, Project, Connection, Table or Recent query. */
  kind: string;
  /** The kind before translation, to pick its icon. */
  kindId: string;
  label: string;
  detail?: string;
  shortcut?: string;
  /** Listed but not run: a disabled action, its reason as the detail. */
  disabled?: boolean;
  run: () => void | Promise<void>;
}

interface Props {
  connections: SavedConnection[];
  /** Tables and columns of the connections loaded so far, by connection id. */
  catalogs: Record<string, CompletionCatalog>;
  onClose: () => void;
}

/** The label with its matched characters bold. */
function Highlight({ text, match }: { text: string; match: FuzzyMatch }) {
  return (
    <>
      {highlightRuns(text, match.positions).map((run, i) => (run.marked ? <b key={i}>{run.text}</b> : <Fragment key={i}>{run.text}</Fragment>))}
    </>
  );
}

/** Opens `schema.table` of `connection` (`database`, `null` for the saved one) in a data
 * viewer, finding whether it is a view. */
async function openTable(
  connection: SavedConnection,
  database: string | null,
  catalog: CompletionCatalog,
  schema: string,
  table: string,
) {
  const current = schema === catalog.default_schema;
  const target = qualifiedName(connection.driver, { name: schema, current }, table);
  let kind: "table" | "view" = "table";
  let hiddenKey: string | null = null;
  try {
    const id = await workbench().sideSession(connection, database);
    if (id !== null) {
      const found = (await api.listObjects(id, schema)).find((o) => o.name === table);
      if (found && found.kind !== "table") kind = "view";
      hiddenKey = found?.hidden_key ?? null;
    }
  } catch {
    // Opened as a table; the editor checks the keys again.
  }
  workbench().openTab("query", connection.id, {
    sql: viewerSql(connection.driver, target, "", hiddenKey),
    autorun: true,
    title: table,
    table: target,
    tableRef: { schema, name: table, kind },
    database,
  });
}

/** Ctrl+K: actions, connections, tables and recent queries, found by typing a few letters. */
export function CommandPalette({ connections, catalogs, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [scripts, setScripts] = useState<ScriptList | null>(null);
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    // In a project, its connections' queries only: the limit counts them alone.
    const connectionIds = workbench().activeProject() ? connections.map((c) => c.id) : undefined;
    api.searchHistory({ connectionIds, limit: 30 }).then(setHistory, () => setHistory([]));
    api.listScripts(workbench().activeProject()?.id ?? null).then(setScripts, () => setScripts(null));
  }, []);

  const items = useMemo((): Item[] => {
    const byId = new Map(connections.map((c) => [c.id, c]));
    const actions = actionsFor("global", undefined).filter((a) => !a.hidden && a.id !== "app.palette");
    return [
      ...actions.map((a) => {
        const reason = a.disabled?.(undefined);
        return {
          key: `action:${a.id}`,
          kind: t("Action"),
          kindId: "Action",
          label: t(a.title),
          detail: reason ? t(reason) : undefined,
          shortcut: shortcutLabel(a),
          disabled: !!reason,
          run: () => runAction(a, undefined, (e) => workbench().showError(toCommandError(e).message)),
        };
      }),
      // Projects, to switch to; All connections first, as in the rail.
      ...[{ id: null, name: t("All connections") }, ...workbench().projects()].map((p) => ({
        key: `project:${projectKey(p.id)}`,
        kind: t("Project"),
          kindId: "Project",
        label: p.name,
        run: () => workbench().switchProject(p.id),
      })),
      ...connections.map((c) => ({
        key: `connection:${c.id}`,
        kind: t("Connection"),
          kindId: "Connection",
        label: t("Open SQL editor: {name}", { name: c.name }),
        detail: isEmbedded(c.driver) ? c.database : `${c.host}/${c.database}`,
        run: () => workbench().openTab("query", c.id),
      })),
      ...connections.map((c) => ({
        key: `duplicate:${c.id}`,
        kind: t("Connection"),
          kindId: "Connection",
        label: t("Duplicate connection: {name}", { name: c.name }),
        run: () => workbench().duplicateConnection(c).catch((e: unknown) => workbench().showError(toCommandError(e).message)),
      })),
      ...Object.entries(catalogs).flatMap(([key, catalog]) => {
        const { connectionId, database } = parseDbKey(key);
        const connection = byId.get(connectionId);
        if (!connection) return [];
        return Object.entries(catalog.schemas).flatMap(([schema, tables]) =>
          Object.keys(tables).map((table) => ({
            key: `table:${connectionId}:${database ?? ""}:${schema}.${table}`,
            kind: t("Table or view"),
          kindId: "Table or view",
            label: schema === catalog.default_schema ? table : `${schema}.${table}`,
            detail: database ? `${connection.name} · ${database}` : connection.name,
            run: () => openTable(connection, database, catalog, schema, table),
          })),
        );
      }),
      // Saved scripts, found by name; Enter opens one on its connection.
      ...(scripts?.entries ?? []).flatMap((e) => {
        const connection = connectionFor(e, connections);
        if (e.folder || !connection) return [];
        return [
          {
            key: `script:${e.path}`,
            kind: t("Script"),
            kindId: "Script",
            label: nameOf(e.path).replace(/\.sql$/i, ""),
            detail: connection.name,
            run: () => openScript(scripts!.root, e, connection),
          },
        ];
      }),
      ...history.flatMap((h) => {
        const connection = byId.get(h.connection_id);
        if (!connection) return [];
        return [
          {
            key: `history:${h.id}`,
            kind: t("Recent query"),
          kindId: "Recent query",
            label: h.sql.replace(/\s+/g, " ").trim().slice(0, 140),
            detail: connection.name,
            run: () => workbench().openTab("query", connection.id, { sql: h.sql }),
          },
        ];
      }),
    ];
  }, [connections, catalogs, history, scripts]);

  const results = useMemo(() => {
    const scored = items.flatMap((item) => {
      const match = fuzzy(query, item.label);
      return match ? [{ item, match }] : [];
    });
    if (query.trim()) scored.sort((a, b) => b.match.score - a.match.score);
    const top = scored.slice(0, LIMIT);
    // Grouped by kind, the group with the best match first; the order inside a group is kept.
    const kinds = [...new Set(top.map((r) => r.item.kind))];
    return kinds.flatMap((kind) => top.filter((r) => r.item.kind === kind));
  }, [items, query]);

  useEffect(() => setSelected(0), [query]);
  useEffect(() => {
    list.current?.children[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  function choose(index: number) {
    const chosen = results[index];
    if (!chosen || chosen.item.disabled) return;
    onClose();
    void Promise.resolve(chosen.item.run()).catch((e: unknown) => workbench().showError(toCommandError(e).message));
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      setSelected((s) => (results.length ? (s + step + results.length) % results.length : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(selected);
    }
  }

  return (
    <div className="palette-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-label={t("Command palette")}>
        <input
          autoFocus
          value={query}
          placeholder={t("Search actions, connections, tables and recent queries…")}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
        />
        <ul ref={list} role="listbox">
          {results.map(({ item, match }, i) => (
            <li
              key={item.key}
              role="option"
              aria-selected={i === selected}
              aria-disabled={item.disabled || undefined}
              className={[i === selected ? "selected" : "", item.kind !== results[i - 1]?.item.kind ? "group-start" : ""].join(" ").trim() || undefined}
              data-group={item.kind}
              onMouseMove={() => setSelected(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(i);
              }}
            >
              <Icon name={KIND_ICONS[item.kindId] ?? "sql"} size={14} />
              <span className="palette-label">
                <Highlight text={item.label} match={match} />
              </span>
              {item.detail && <span className="palette-detail">{item.detail}</span>}
              {item.shortcut && <kbd>{item.shortcut}</kbd>}
            </li>
          ))}
          {results.length === 0 && <li className="palette-empty muted">{t("Nothing matches.")}</li>}
        </ul>
        <div className="palette-hint">
          <span>
            <kbd>↑↓</kbd>
            {t("to choose")}
          </span>
          <span>
            <kbd>↵</kbd>
            {t("open")}
          </span>
          <span>
            <kbd>Esc</kbd>
            {t("close")}
          </span>
          <span className="palette-count">{t("{n} results", { n: results.length })}</span>
        </div>
      </div>
    </div>
  );
}
