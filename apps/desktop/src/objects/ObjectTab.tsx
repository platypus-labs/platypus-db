import { createElement, useEffect, useState } from "react";
import {
  api,
  type CatalogKey,
  type ColumnUsage,
  type ColumnUse,
  type DriverKind,
  type ObjectKind,
  type ObjectTarget,
  type Property,
  type TableDetails,
  toCommandError,
} from "../api";
import { isRelation } from "../components/Navigator";
import { catalogKey, reportCatalogError, sensitiveMark, setSensitiveMark, useCatalogVersion } from "../catalog/catalog";
import { useSensitive } from "../guards/mask";
import { CatalogTags, NotesPanel, useNoteDraft } from "../catalog/NotesPanel";
import { Icon, objectIcon } from "../ui/Icon";
import { SqlEditor } from "../editor/SqlEditor";
import { onNavigatorRefresh, requestNavigatorRefresh } from "../navigatorEvents";
import { qualifiedName, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { editsTables, openObjectTab, openTableEditor, showsData } from "./open";
import { openScriptFile, scriptFile } from "../scripts/scripts";
import { SqlActionDialog } from "./SqlActionDialog";
import { t, msg } from "../i18n";

export type ObjectSection =
  | "properties"
  | "columns"
  | "indexes"
  | "constraints"
  | "foreign_keys"
  | "references"
  | "usage"
  | "ddl";

const NO_COMMENTS: DriverKind[] = ["sqlite", "libsql", "mongo", "redis", "elasticsearch", "cassandra"];

/** Details of one database object: properties, structure and DDL. */
export interface ObjectTabModel extends TabBase {
  kind: "object";
  schema: string;
  /** Whether `schema` is the session's current one: names are then left unqualified. */
  currentSchema: boolean;
  name: string;
  objectKind: ObjectKind;
  /** The table of a trigger, or the partitioned table of a partition. */
  parent: string | null;
  section?: ObjectSection;
}

declare module "../tabs/registry" {
  interface TabKinds {
    object: ObjectTabModel;
  }
}

const KIND_LABELS: Record<ObjectKind, string> = {
  table: msg("Table"),
  view: msg("View"),
  materialized_view: msg("Materialized view"),
  foreign_table: msg("Foreign table"),
  sequence: msg("Sequence"),
  function: msg("Function"),
  procedure: msg("Procedure"),
  trigger: msg("Trigger"),
  type: msg("Type"),
  domain: msg("Domain"),
  extension: msg("Extension"),
  event: msg("Event"),
  dictionary: msg("Dictionary"),
  synonym: msg("Synonym"),
  package: msg("Package"),
};

const SECTION_LABELS: Record<ObjectSection, string> = {
  properties: msg("Properties"),
  columns: msg("Columns"),
  indexes: msg("Indexes"),
  constraints: msg("Constraints"),
  foreign_keys: msg("Foreign keys"),
  references: msg("References"),
  usage: msg("Used by"),
  ddl: "DDL",
};

function sectionsFor(kind: ObjectKind): ObjectSection[] {
  if (kind === "table" || kind === "foreign_table") {
    return ["properties", "columns", "indexes", "constraints", "foreign_keys", "references", "usage", "ddl"];
  }
  if (kind === "view" || kind === "materialized_view" || kind === "dictionary") {
    return ["properties", "columns", "ddl"];
  }
  return ["properties", "ddl"];
}

interface Loaded {
  properties: Property[];
  details: TableDetails | null;
  /** `null` when the driver cannot print DDL. */
  ddl: string | null;
  /** The comment of a table or view, from the object list as every driver fills it. */
  comment: string | null;
}

async function loadObject(sid: number, tab: ObjectTabModel, commentable: boolean): Promise<Loaded> {
  const target: ObjectTarget = { schema: tab.schema, name: tab.name, kind: tab.objectKind, parent: tab.parent };
  const relation = isRelation(tab.objectKind);
  const ddlSupported = workbench().capabilities(sid)?.object_ddl ?? true;
  const [properties, details, ddl, comment] = await Promise.all([
    api.objectProperties(sid, target),
    relation ? api.tableDetails(sid, tab.schema, tab.name) : Promise.resolve(null),
    ddlSupported ? api.objectDdl(sid, target) : Promise.resolve(null),
    relation && commentable
      ? api.listObjects(sid, tab.schema).then((list) => list.find((o) => o.name === tab.name && isRelation(o.kind))?.comment ?? null)
      : Promise.resolve(null),
  ]);
  return { properties, details, ddl, comment };
}

export function ObjectTab({ tab, connection, active, sideSession, onChange }: TabProps<ObjectTabModel>) {
  const [data, setData] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const [tagSuggestions, setTagSuggestions] = useState<string[]>([]);
  // Also re-renders the note and tag cells of the columns, which read the catalog as they draw.
  const catalogVersion = useCatalogVersion();
  // What the rules on names and types say of each column, before the user's marks.
  const byRules = useSensitive();
  const ckey = (column = "") => catalogKey(tab.database, tab.schema, tab.name, column);
  const sections = sectionsFor(tab.objectKind);
  const section = tab.section && sections.includes(tab.section) ? tab.section : sections[0]!;
  const qualified = qualifiedName(connection.driver, { name: tab.schema, current: tab.currentSchema }, tab.name);

  // The database keeps comments; the app writes those of tables (the statements it generates
  // are table ones), and shows the others.
  const commentable = !NO_COMMENTS.includes(connection.driver);
  const editable = commentable && tab.objectKind === "table";

  function editComment(column: string | null, current: string | null) {
    workbench().openDialog((close) =>
      createElement(SqlActionDialog, {
        connection,
        database: tab.database ?? null,
        title: t("Comment of {name}", { name: column ?? tab.name }),
        description: t("Written to the database, visible to everyone who reads it."),
        confirm: t("Save comment"),
        danger: false,
        options: { rename: current ?? "", renameLabel: t("Comment"), allowEmpty: true, multiline: true },
        generate: async ({ newName }) => {
          const sid = await sideSession();
          return sid === null ? null : api.commentSql(sid, tab.schema, tab.name, column, newName || null);
        },
        // Reloads this tab too, and the navigator's tooltips.
        onDone: () => requestNavigatorRefresh({ connectionId: connection.id, database: tab.database, schema: tab.schema }),
        onClose: close,
      }),
    );
  }

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setError(null);
      const sid = await sideSession();
      if (cancelled) return;
      // The password prompt was cancelled or the connection failed: say so, not "Loading…".
      if (sid === null) return setError(t("Not connected."));
      try {
        const loaded = await loadObject(sid, tab, commentable);
        if (!cancelled) setData(loaded);
      } catch (e) {
        if (!cancelled) setError(toCommandError(e).message);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [version]);

  // The tags in use, offered to the tag fields; again after a save, which may add one.
  useEffect(() => {
    let stale = false;
    const show = (tags: string[]) => {
      if (!stale) setTagSuggestions(tags);
    };
    api.catalogTags(tab.connectionId).then((t) => show(t.map((x) => x.tag)), () => show([]));
    return () => {
      stale = true;
    };
  }, [tab.connectionId, catalogVersion]);

  // Reload when the object's schema changes, e.g. after an ALTER from the navigator.
  useEffect(
    () =>
      onNavigatorRefresh((r) => {
        if (r.connectionId === tab.connectionId && (r.database ?? null) === (tab.database ?? null)) {
          if (r.schema === undefined || r.schema === tab.schema) setVersion((v) => v + 1);
        }
      }),
    [tab.connectionId, tab.database, tab.schema],
  );

  function openReferenced(schema: string, table: string) {
    openObjectTab(connection.id, {
      database: tab.database,
      schema,
      currentSchema: schema === tab.schema ? tab.currentSchema : false,
      name: table,
      kind: "table",
      parent: null,
    });
  }

  const d = data?.details;
  const relation = isRelation(tab.objectKind);
  const showComment = commentable && relation;
  const objectComment = (
    <>
      {data?.comment}{" "}
      {editable && (
        <button className="link" onClick={() => editComment(null, data?.comment ?? null)}>
          {t("Edit")}
        </button>
      )}
    </>
  );

  return (
    <div className="object-tab" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon" data-kind={objectIcon(tab.objectKind)}>
          <Icon name={objectIcon(tab.objectKind)} size={16} />
        </span>
        <div className="object-title">
          <strong>{tab.name}</strong>
          <span className="muted small">
            {t("{kind} in {schema}", { kind: t(KIND_LABELS[tab.objectKind]), schema: tab.schema })}
            {tab.parent ? ` · ${t(tab.objectKind === "trigger" ? "on {table}" : "partition of {table}", { table: tab.parent })}` : ""}
            {tab.database ? ` · ${tab.database}` : ""}
          </span>
        </div>
        <span className="spacer" />
        {relation && (
          <button
            onClick={() =>
              workbench().focusTab((t) => showsData(t, connection.id, tab.database, qualified)) ||
              workbench().openTab("query", connection.id, {
                sql: viewerSql(connection.driver, qualified),
                autorun: true,
                title: tab.name,
                table: qualified,
                tableRef: { schema: tab.schema, name: tab.name, kind: tab.objectKind },
                database: tab.database,
              })
            }
          >
            {t("Open data")}
          </button>
        )}
        {/* A partition takes its columns and keys from its table. */}
        {tab.objectKind === "table" && !tab.parent && editsTables(connection.driver) && (
          <button
            onClick={() => openTableEditor(connection.id, { database: tab.database, schema: tab.schema, name: tab.name })}
            title={t("Edit the columns, keys, indexes and constraints of the table")}
          >
            {t("Edit table…")}
          </button>
        )}
        {data?.ddl && (
          <button
            onClick={() =>
              workbench().openTab("query", connection.id, {
                sql: data.ddl!,
                title: t("{name} DDL", { name: tab.name }),
                database: tab.database,
              })
            }
          >
            {t("Edit DDL in editor")}
          </button>
        )}
        <button className="square" onClick={() => setVersion((v) => v + 1)} title={t("Refresh from the database")} aria-label={t("Refresh")}>
          <Icon name="refresh" />
        </button>
      </header>
      {relation && (
        <NotesPanel
          connectionId={connection.id}
          catalogKey={ckey()}
          suggestions={tagSuggestions}
          columns={tab.objectKind === "table" ? d?.columns.map((c) => c.name) : undefined}
        />
      )}
      <nav className="object-sections">
        {sections.map((s) => (
          <button key={s} className={s === section ? "active" : ""} onClick={() => onChange({ section: s })}>
            {t(SECTION_LABELS[s])}
            {d && s !== "properties" && s !== "ddl" && <span className="count">{count(d, s)}</span>}
          </button>
        ))}
      </nav>
      <div className="object-body">
        {error && <div className="notice error">{error}</div>}
        {!data && !error && <div className="muted small">{t("Loading…")}</div>}
        {data && section === "properties" && (
          <MetaTable
            head={[t("Property"), t("Value")]}
            rows={[
              [t("Name"), tab.name],
              [t("Kind"), t(KIND_LABELS[tab.objectKind])],
              [t("Schema"), tab.schema],
              ...(tab.parent ? [[t(tab.objectKind === "trigger" ? "Table" : "Partition of"), tab.parent]] : []),
              ...(showComment ? [[t("Comment"), objectComment]] : []),
              // Some drivers list the comment among the properties: it is shown once, above.
              ...data.properties.filter((p) => !(showComment && p.name === "Comment")).map((p) => [p.name, p.value]),
            ]}
          />
        )}
        {d && section === "columns" && (
          <MetaTable
            head={["#", t("Name"), t("Type"), t("Nullable"), t("Default"), t("Key"), t("Comment (database)"), t("Note"), t("Tags"), t("Sensitive")]}
            rows={d.columns.map((c, i) => [
              String(i + 1),
              c.name,
              c.data_type,
              t(c.nullable ? "yes" : "no"),
              c.default ?? "",
              c.primary_key ? "PK" : "",
              editable ? (
                <button key="comment" className="link" onClick={() => editComment(c.name, c.comment)}>
                  {c.comment || t("Edit")}
                </button>
              ) : (
                <span key="comment" title={commentable ? undefined : t("This database has no comments: use a note")}>
                  {c.comment ?? ""}
                </span>
              ),
              // Keyed by column: after a reload that moves the columns, a field being typed in
              // does not pass to another one.
              <ColumnNote key={`note ${c.name}`} connectionId={connection.id} catalogKey={ckey(c.name)} />,
              <CatalogTags
                key={`tags ${c.name}`}
                label={t("Tags of {column}", { column: c.name })}
                connectionId={connection.id}
                catalogKey={ckey(c.name)}
                suggestions={tagSuggestions}
              />,
              <SensitiveCell
                key={`sensitive ${c.name}`}
                connectionId={connection.id}
                catalogKey={ckey(c.name)}
                byRules={byRules({ name: c.name, db_type: c.data_type })}
              />,
            ])}
          />
        )}
        {d && section === "indexes" && (
          <MetaTable
            head={[t("Name"), t("Columns"), t("Unique"), t("Primary")]}
            rows={d.indexes.map((i) => [i.name, i.columns.join(", "), i.unique ? t("yes") : "", i.primary ? t("yes") : ""])}
          />
        )}
        {d && section === "constraints" && (
          <MetaTable
            head={[t("Name"), t("Kind"), t("Columns"), t("Definition")]}
            rows={d.constraints.map((c) => [c.name, c.kind.replace("_", " "), c.columns.join(", "), c.definition])}
          />
        )}
        {d && section === "foreign_keys" && (
          <MetaTable
            head={[t("Name"), t("Columns"), t("References"), t("Referenced columns")]}
            rows={d.foreign_keys.map((f) => [
              f.name,
              f.columns.join(", "),
              <button key="ref" className="link" onClick={() => openReferenced(f.ref_schema, f.ref_table)}>
                {f.ref_schema === tab.schema ? f.ref_table : `${f.ref_schema}.${f.ref_table}`}
              </button>,
              f.ref_columns.join(", "),
            ])}
          />
        )}
        {d && section === "references" && (
          <MetaTable
            head={[t("Table"), t("Foreign key"), t("Columns"), t("Referenced columns")]}
            rows={d.referenced_by.map((r) => [
              <button key="ref" className="link" onClick={() => openReferenced(r.schema, r.table)}>
                {r.schema === tab.schema ? r.table : `${r.schema}.${r.table}`}
              </button>,
              r.name,
              r.columns.join(", "),
              r.ref_columns.join(", "),
            ])}
          />
        )}
        {d && section === "usage" && (
          <ColumnUsageSection
            connectionId={connection.id}
            schema={tab.schema}
            currentSchema={tab.currentSchema}
            database={tab.database}
            table={tab.name}
            columns={d.columns.map((c) => c.name)}
            version={version}
            sideSession={sideSession}
          />
        )}
        {data && section === "ddl" &&
          (data.ddl === null ? (
            <div className="notice muted">{t("This driver cannot show the DDL of objects.")}</div>
          ) : (
            <div className="object-ddl">
              <SqlEditor key={data.ddl} initialValue={data.ddl} driver={connection.driver} readOnly />
            </div>
          ))}
      </div>
    </div>
  );

  function count(details: TableDetails, s: ObjectSection) {
    switch (s) {
      case "columns":
        return details.columns.length;
      case "indexes":
        return details.indexes.length;
      case "constraints":
        return details.constraints.length;
      case "foreign_keys":
        return details.foreign_keys.length;
      case "references":
        return details.referenced_by.length;
      default:
        return null;
    }
  }
}

function MetaTable({ head, rows }: { head: string[]; rows: React.ReactNode[][] }) {
  if (rows.length === 0) return <div className="muted small">{t("None")}</div>;
  return (
    <table className="meta-table">
      <thead>
        <tr>
          {head.map((h) => (
            <th key={h}>{h}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i}>
            {r.map((cell, j) => (
              <td key={j}>{cell}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Where the columns of a table are used: views, routines and triggers of its schema, and the
 *  saved scripts of the active project. Read when shown, and again on Refresh. */
function ColumnUsageSection(props: {
  connectionId: string;
  schema: string;
  currentSchema: boolean;
  database: string | null | undefined;
  table: string;
  columns: string[];
  version: number;
  sideSession: () => Promise<number | null>;
}) {
  const { connectionId, schema, table, columns } = props;
  const [usage, setUsage] = useState<ColumnUsage | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setUsage(null);
    setError(null);
    void (async () => {
      try {
        const sid = await props.sideSession();
        if (sid === null) {
          if (live) setError(t("Not connected."));
          return;
        }
        const read = await api.columnUsage(sid, schema, table, columns, workbench().activeProject()?.id ?? null);
        if (live) setUsage(read);
      } catch (e) {
        if (live) setError(toCommandError(e).message);
      }
    })();
    return () => {
      live = false;
    };
  }, [props.version, columns.join("\n")]);

  if (error) return <div className="notice error">{error}</div>;
  if (!usage) return <div className="muted small">{t("Loading…")}</div>;
  const unused = columns.filter((c) => !usage.uses.some((u) => u.column === c));
  const open = (u: ColumnUse) =>
    u.kind
      ? openObjectTab(connectionId, { database: props.database, schema, currentSchema: props.currentSchema, name: u.name, kind: u.kind, parent: u.parent })
      : usage.scripts_root && void openScriptFile(scriptFile(usage.scripts_root, u.name), connectionId).catch((e: unknown) => workbench().showError(toCommandError(e).message));
  return (
    <>
      <MetaTable
        head={[t("Column"), t("Used by"), t("Kind"), t("Line")]}
        rows={usage.uses.map((u) => [
          u.column,
          <button key="use" className="link" onClick={() => open(u)} title={u.excerpt}>
            {u.kind === "trigger" && u.parent ? `${u.name} (${u.parent})` : u.name}
          </button>,
          u.kind ? t(KIND_LABELS[u.kind]) : t("Saved script"),
          <span key="line" title={u.excerpt}>
            {u.line}: <code>{u.excerpt}</code>
          </span>,
        ])}
      />
      {unused.length > 0 && (
        <p className="muted small" data-field="unused">
          {t("Not named by any view, routine, trigger or saved script: {columns}", { columns: unused.join(", ") })}
        </p>
      )}
    </>
  );
}

/** Whether a column is sensitive: as the rules say, or the user's mark over them. */
function SensitiveCell({ connectionId, catalogKey, byRules }: { connectionId: string; catalogKey: CatalogKey; byRules: boolean }) {
  const mark = sensitiveMark(connectionId, catalogKey);
  return (
    <select
      className="cell-input"
      aria-label={t("Sensitive: {column}", { column: catalogKey.column })}
      value={mark === undefined ? "" : mark ? "yes" : "no"}
      onChange={(e) => setSensitiveMark(connectionId, catalogKey, e.target.value === "" ? null : e.target.value === "yes").catch(reportCatalogError)}
    >
      <option value="">{t(byRules ? "Sensitive, by the rules" : "Not sensitive, by the rules")}</option>
      <option value="yes">{t("Sensitive")}</option>
      <option value="no">{t("Not sensitive")}</option>
    </select>
  );
}

function ColumnNote({ connectionId, catalogKey }: { connectionId: string; catalogKey: CatalogKey }) {
  const note = useNoteDraft(connectionId, catalogKey);
  return <input className="cell-input" aria-label={t("Note of {column}", { column: catalogKey.column })} {...note} />;
}
