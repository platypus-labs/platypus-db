import { useEffect, useMemo, useRef, useState } from "react";
import { actionsFor, runAction, shortcutLabel } from "../actions";
import { api, type CompletionCatalog, type HistoryEntry, isEmbedded, type SavedConnection, toCommandError } from "../api";
import { projectKey } from "../projects/state";
import { qualifiedName, viewerSql } from "../sqlNames";
import { parseDbKey, workbench } from "../workbench";
import { type FuzzyMatch, fuzzy } from "./fuzzy";
import { t } from "../i18n";

/** Results shown at most. */
const LIMIT = 60;

interface Item {
  key: string;
  /** What it is, as shown: Action, Project, Connection, Table or Recent query. */
  kind: string;
  label: string;
  detail?: string;
  shortcut?: string;
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
  const marked = new Set(match.positions);
  return (
    <>
      {[...text].map((c, i) => (marked.has(i) ? <b key={i}>{c}</b> : c))}
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
  try {
    const id = await workbench().sideSession(connection, database);
    if (id !== null) {
      const found = (await api.listObjects(id, schema)).find((o) => o.name === table);
      if (found && found.kind !== "table") kind = "view";
    }
  } catch {
    // Opened as a table; the editor checks the keys again.
  }
  workbench().openTab("query", connection.id, {
    sql: viewerSql(connection.driver, target),
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
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    // In a project, its connections' queries only: the limit counts them alone.
    const connectionIds = workbench().activeProject() ? connections.map((c) => c.id) : undefined;
    api.searchHistory({ connectionIds, limit: 30 }).then(setHistory, () => setHistory([]));
  }, []);

  const items = useMemo((): Item[] => {
    const byId = new Map(connections.map((c) => [c.id, c]));
    const actions = actionsFor("global", undefined).filter((a) => !a.hidden && a.id !== "app.palette");
    return [
      ...actions.map((a) => ({
        key: `action:${a.id}`,
        kind: t("Action"),
        label: t(a.title),
        shortcut: shortcutLabel(a),
        run: () => runAction(a, undefined, (e) => workbench().showError(toCommandError(e).message)),
      })),
      // Projects, to switch to; All connections first, as in the rail.
      ...[{ id: null, name: t("All connections") }, ...workbench().projects()].map((p) => ({
        key: `project:${projectKey(p.id)}`,
        kind: t("Project"),
        label: p.name,
        run: () => workbench().switchProject(p.id),
      })),
      ...connections.map((c) => ({
        key: `connection:${c.id}`,
        kind: t("Connection"),
        label: t("Open SQL editor: {name}", { name: c.name }),
        detail: isEmbedded(c.driver) ? c.database : `${c.host}/${c.database}`,
        run: () => workbench().openTab("query", c.id),
      })),
      ...Object.entries(catalogs).flatMap(([key, catalog]) => {
        const { connectionId, database } = parseDbKey(key);
        const connection = byId.get(connectionId);
        if (!connection) return [];
        return Object.entries(catalog.schemas).flatMap(([schema, tables]) =>
          Object.keys(tables).map((table) => ({
            key: `table:${connectionId}:${database ?? ""}:${schema}.${table}`,
            kind: t("Table"),
            label: schema === catalog.default_schema ? table : `${schema}.${table}`,
            detail: database ? `${connection.name} · ${database}` : connection.name,
            run: () => openTable(connection, database, catalog, schema, table),
          })),
        );
      }),
      ...history.flatMap((h) => {
        const connection = byId.get(h.connection_id);
        if (!connection) return [];
        return [
          {
            key: `history:${h.id}`,
            kind: t("Recent query"),
            label: h.sql.replace(/\s+/g, " ").trim().slice(0, 140),
            detail: connection.name,
            run: () => workbench().openTab("query", connection.id, { sql: h.sql }),
          },
        ];
      }),
    ];
  }, [connections, catalogs, history]);

  const results = useMemo(() => {
    const scored = items.flatMap((item) => {
      const match = fuzzy(query, item.label);
      return match ? [{ item, match }] : [];
    });
    if (query.trim()) scored.sort((a, b) => b.match.score - a.match.score);
    return scored.slice(0, LIMIT);
  }, [items, query]);

  useEffect(() => setSelected(0), [query]);
  useEffect(() => {
    list.current?.children[selected]?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  function choose(index: number) {
    const chosen = results[index];
    if (!chosen) return;
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
              className={i === selected ? "selected" : undefined}
              onMouseMove={() => setSelected(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(i);
              }}
            >
              <span className="palette-kind">{item.kind}</span>
              <span className="palette-label">
                <Highlight text={item.label} match={match} />
              </span>
              {item.detail && <span className="palette-detail">{item.detail}</span>}
              {item.shortcut && <kbd>{item.shortcut}</kbd>}
            </li>
          ))}
          {results.length === 0 && <li className="palette-empty muted">{t("Nothing matches.")}</li>}
        </ul>
      </div>
    </div>
  );
}
