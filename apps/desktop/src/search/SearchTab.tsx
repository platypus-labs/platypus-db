import { useRef, useState } from "react";
import { api, type MetadataMatch, type ObjectKind, type SchemaMeta, type TableHits, toCommandError } from "../api";
import { qualifiedName, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { Icon, objectIcon } from "../ui/Icon";
import { workbench } from "../workbench";

export type SearchMode = "metadata" | "data";

/** Searches a database: object names, columns and sources, or the text of table rows. */
export interface SearchTabModel extends TabBase {
  kind: "search";
  mode: SearchMode;
  query: string;
  /** The schema whose tables a data search reads; `null`: every user schema. */
  schema: string | null;
}

declare module "../tabs/registry" {
  interface TabKinds {
    search: SearchTabModel;
  }
}

interface TableHit extends TableHits {
  schema: SchemaMeta;
  table: string;
  kind: ObjectKind;
}

type Results =
  | { mode: "metadata"; hits: MetadataMatch[]; schemas: SchemaMeta[] }
  | { mode: "data"; hits: TableHit[]; searched: number; failed: number };

export function SearchTab({ tab, connection, active, ensureSession, sideSession, onChange }: TabProps<SearchTabModel>) {
  const [results, setResults] = useState<Results | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The table being read, while a data search runs. */
  const [running, setRunning] = useState<string | null>(null);
  const stop = useRef(false);

  async function searchMetadata(needle: string) {
    const sid = await sideSession();
    if (sid === null) return;
    const [hits, schemas] = await Promise.all([api.searchMetadata(sid, needle), api.listSchemas(sid)]);
    setResults({ mode: "metadata", hits, schemas });
  }

  /** Table by table on the tab's own session, so the navigator never waits behind it. */
  async function searchData(needle: string) {
    const sid = await ensureSession();
    if (sid === null) return;
    // Production sessions start in manual commit, where these reads would leave a
    // transaction open; the search never writes.
    await api.setAutocommit(sid, true).catch(() => {});
    const schemas = (await api.listSchemas(sid)).filter((s) => (tab.schema === null ? !s.system : s.name === tab.schema));
    const found: Results & { mode: "data" } = { mode: "data", hits: [], searched: 0, failed: 0 };
    setResults({ ...found });
    for (const schema of schemas) {
      // Partitions are read through their table.
      const tables = (await api.listObjects(sid, schema.name)).filter(
        (o) => (o.kind === "table" && o.parent === null) || o.kind === "materialized_view",
      );
      for (const table of tables) {
        if (stop.current) return;
        setRunning(`${schema.name}.${table.name}`);
        try {
          const hit = await api.searchTableData(sid, schema.name, table.name, needle);
          if (hit.rows > 0) found.hits.push({ schema, table: table.name, kind: table.kind, ...hit });
          found.searched++;
        } catch {
          found.failed++;
        }
        setResults({ ...found, hits: [...found.hits] });
      }
    }
  }

  async function search() {
    const needle = tab.query.trim();
    if (!needle || running !== null) return;
    stop.current = false;
    setError(null);
    setResults(null);
    setRunning("");
    try {
      await (tab.mode === "metadata" ? searchMetadata(needle) : searchData(needle));
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setRunning(null);
    }
  }

  function openHit(hit: MetadataMatch, schemas: SchemaMeta[]) {
    workbench().openTab("object", connection.id, {
      database: tab.database,
      schema: hit.schema,
      currentSchema: schemas.some((s) => s.current && s.name === hit.schema),
      name: hit.name,
      objectKind: hit.kind,
      parent: hit.parent,
      title: hit.name,
      section: hit.matched === "column" ? "columns" : hit.matched === "source" ? "ddl" : undefined,
    });
  }

  function openRows(hit: TableHit) {
    const target = qualifiedName(connection.driver, hit.schema, hit.table);
    workbench().openTab("query", connection.id, {
      sql: viewerSql(connection.driver, target, hit.condition, null),
      autorun: true,
      title: hit.table,
      table: target,
      tableRef: { schema: hit.schema.name, name: hit.table, kind: hit.kind },
      filter: hit.condition,
      database: tab.database,
    });
  }

  const scope = tab.database ?? connection.name;
  return (
    <div className="object-tab search-tab" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon">
          <Icon name="search" size={20} />
        </span>
        <form
          className="search-form"
          onSubmit={(e) => {
            e.preventDefault();
            void search();
          }}
        >
          <input
            autoFocus
            className="search-input"
            placeholder={tab.mode === "metadata" ? "Name, column or text in a source…" : "Text in any column…"}
            value={tab.query}
            onChange={(e) => onChange({ query: e.target.value })}
          />
          {running === null ? (
            <button type="submit" className="primary" disabled={!tab.query.trim()}>
              Search
            </button>
          ) : (
            <button type="button" disabled={tab.mode === "metadata"} onClick={() => (stop.current = true)}>
              Stop
            </button>
          )}
        </form>
      </header>
      <nav className="object-sections">
        {(["metadata", "data"] as const).map((mode) => (
          <button
            key={mode}
            className={mode === tab.mode ? "active" : ""}
            disabled={running !== null}
            onClick={() => {
              onChange({ mode });
              setResults(null);
            }}
          >
            {mode === "metadata" ? "Objects" : "Data"}
          </button>
        ))}
      </nav>
      <div className="object-body">
        <p className="muted small">
          {tab.mode === "metadata" ? (
            <>Names of objects and columns, and sources of views, routines and triggers, in {scope}.</>
          ) : (
            <>
              Rows of every table in {tab.schema === null ? `the user schemas of ${scope}` : `${scope} · ${tab.schema}`}{" "}
              that contain the text in any column. Each table is read in full.
              {tab.schema !== null && running === null && (
                <>
                  {" "}
                  <button className="link" onClick={() => onChange({ schema: null })}>
                    Search all schemas
                  </button>
                </>
              )}
            </>
          )}
        </p>
        {error && <div className="notice error">{error}</div>}
        {running && <div className="muted small">Reading {running}…</div>}
        {results?.mode === "metadata" && <MetadataResults results={results} onOpen={openHit} />}
        {results?.mode === "data" && <DataResults results={results} done={running === null} onOpen={openRows} />}
      </div>
    </div>
  );
}

function MetadataResults({
  results,
  onOpen,
}: {
  results: Results & { mode: "metadata" };
  onOpen: (hit: MetadataMatch, schemas: SchemaMeta[]) => void;
}) {
  if (results.hits.length === 0) return <div className="muted small">Nothing found.</div>;
  return (
    <table className="meta-table search-results">
      <thead>
        <tr>
          <th>Object</th>
          <th>Schema</th>
          <th>Matched</th>
        </tr>
      </thead>
      <tbody>
        {results.hits.map((h, i) => (
          <tr key={i} tabIndex={0} onClick={() => onOpen(h, results.schemas)} onKeyDown={(e) => e.key === "Enter" && onOpen(h, results.schemas)}>
            <td>
              <span className="tree-icon" data-kind={objectIcon(h.kind)}>
                <Icon name={objectIcon(h.kind)} />
              </span>
              {h.name}
              {h.parent && <span className="muted"> on {h.parent}</span>}
            </td>
            <td>{h.schema}</td>
            <td>{h.column ? `${h.matched} ${h.column}` : h.matched}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function DataResults({
  results,
  done,
  onOpen,
}: {
  results: Results & { mode: "data" };
  done: boolean;
  onOpen: (hit: TableHit) => void;
}) {
  const summary = `${results.searched} tables searched${results.failed ? `, ${results.failed} could not be read` : ""}.`;
  return (
    <>
      {done && <div className="muted small">{results.hits.length === 0 ? `Nothing found; ${summary}` : summary}</div>}
      {results.hits.length > 0 && (
        <table className="meta-table search-results">
          <thead>
            <tr>
              <th>Table</th>
              <th>Rows</th>
            </tr>
          </thead>
          <tbody>
            {results.hits.map((h) => (
              <tr
                key={`${h.schema.name}.${h.table}`}
                tabIndex={0}
                onClick={() => onOpen(h)}
                onKeyDown={(e) => e.key === "Enter" && onOpen(h)}
              >
                <td>
                  <span className="tree-icon" data-kind="table">
                    <Icon name="table" />
                  </span>
                  {h.schema.name}.{h.table}
                </td>
                <td>{h.rows >= h.limit ? `${h.limit}+` : h.rows}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </>
  );
}

