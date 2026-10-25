import { useEffect, useState } from "react";
import {
  api,
  type CatalogKey,
  type ObjectKind,
  type ObjectTarget,
  type Property,
  type TableDetails,
  toCommandError,
} from "../api";
import { isRelation } from "../components/Navigator";
import { catalogEntry, catalogKey, saveCatalog, useCatalogVersion } from "../catalog/catalog";
import { NotesPanel } from "../catalog/NotesPanel";
import { TagInput } from "../catalog/TagInput";
import { Icon, objectIcon } from "../ui/Icon";
import { SqlEditor } from "../editor/SqlEditor";
import { onNavigatorRefresh } from "../navigatorEvents";
import { qualifiedName, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { t, msg } from "../i18n";

export type ObjectSection =
  | "properties"
  | "columns"
  | "indexes"
  | "constraints"
  | "foreign_keys"
  | "references"
  | "ddl";

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
};

const SECTION_LABELS: Record<ObjectSection, string> = {
  properties: msg("Properties"),
  columns: msg("Columns"),
  indexes: msg("Indexes"),
  constraints: msg("Constraints"),
  foreign_keys: msg("Foreign keys"),
  references: msg("References"),
  ddl: "DDL",
};

function sectionsFor(kind: ObjectKind): ObjectSection[] {
  if (kind === "table" || kind === "foreign_table") {
    return ["properties", "columns", "indexes", "constraints", "foreign_keys", "references", "ddl"];
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
}

export function ObjectTab({ tab, connection, active, sideSession, onChange }: TabProps<ObjectTabModel>) {
  const [data, setData] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const [tagSuggestions, setTagSuggestions] = useState<string[]>([]);
  useCatalogVersion();
  const ckey = (column = "") => catalogKey(tab.database, tab.schema, tab.name, column);
  const sections = sectionsFor(tab.objectKind);
  const section = tab.section && sections.includes(tab.section) ? tab.section : sections[0]!;
  const target: ObjectTarget = { schema: tab.schema, name: tab.name, kind: tab.objectKind, parent: tab.parent };
  const qualified = qualifiedName(connection.driver, { name: tab.schema, current: tab.currentSchema }, tab.name);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setError(null);
      const sid = await sideSession();
      if (sid === null || cancelled) return;
      try {
        const relation = isRelation(tab.objectKind);
        const ddlSupported = workbench().capabilities(sid)?.object_ddl ?? true;
        const [properties, details, ddl] = await Promise.all([
          api.objectProperties(sid, target),
          relation ? api.tableDetails(sid, tab.schema, tab.name) : Promise.resolve(null),
          ddlSupported ? api.objectDdl(sid, target) : Promise.resolve(null),
        ]);
        if (!cancelled) setData({ properties, details, ddl });
      } catch (e) {
        if (!cancelled) setError(toCommandError(e).message);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [version]);

  useEffect(() => {
    api.catalogTags(tab.connectionId).then((t) => setTagSuggestions(t.map((x) => x.tag)), () => setTagSuggestions([]));
  }, [tab.connectionId]);

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
    workbench().openTab("object", connection.id, {
      database: tab.database,
      schema,
      currentSchema: schema === tab.schema ? tab.currentSchema : false,
      name: table,
      objectKind: "table",
      parent: null,
      title: table,
    });
  }

  const d = data?.details;

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
        {isRelation(tab.objectKind) && (
          <button
            onClick={() =>
              workbench().openTab("query", connection.id, {
                sql: viewerSql(connection.driver, qualified),
                autorun: true,
                title: tab.name,
                table: qualified,
                database: tab.database,
              })
            }
          >
            {t("Open data")}
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
      {isRelation(tab.objectKind) && (
        <NotesPanel connectionId={connection.id} catalogKey={ckey()} suggestions={tagSuggestions} />
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
              ...data.properties.map((p) => [p.name, p.value]),
            ]}
          />
        )}
        {d && section === "columns" && (
          <MetaTable
            head={["#", t("Name"), t("Type"), t("Nullable"), t("Default"), t("Key"), t("Comment"), t("Note"), t("Tags")]}
            rows={d.columns.map((c, i) => [
              String(i + 1),
              c.name,
              c.data_type,
              t(c.nullable ? "yes" : "no"),
              c.default ?? "",
              c.primary_key ? "PK" : "",
              c.comment ?? "",
              <ColumnNote key="note" connectionId={connection.id} catalogKey={ckey(c.name)} />,
              <ColumnTags key="tags" connectionId={connection.id} catalogKey={ckey(c.name)} suggestions={tagSuggestions} />,
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


const reportError = (e: unknown) => workbench().showError(toCommandError(e).message);

function ColumnNote({ connectionId, catalogKey }: { connectionId: string; catalogKey: CatalogKey }) {
  useCatalogVersion();
  const entry = catalogEntry(connectionId, catalogKey);
  const [note, setNote] = useState(entry?.note ?? "");
  useEffect(() => setNote(entry?.note ?? ""), [entry?.note]);
  return (
    <input
      className="cell-input"
      aria-label={`Note of ${catalogKey.column}`}
      value={note}
      onChange={(e) => setNote(e.target.value)}
      onBlur={() =>
        note !== (entry?.note ?? "") && void saveCatalog(connectionId, catalogKey, note, entry?.tags ?? []).catch(reportError)
      }
    />
  );
}

function ColumnTags({
  connectionId,
  catalogKey,
  suggestions,
}: {
  connectionId: string;
  catalogKey: CatalogKey;
  suggestions: string[];
}) {
  useCatalogVersion();
  const entry = catalogEntry(connectionId, catalogKey);
  return (
    <TagInput
      label={`Tags of ${catalogKey.column}`}
      tags={entry?.tags ?? []}
      suggestions={suggestions}
      onChange={(tags) => void saveCatalog(connectionId, catalogKey, entry?.note ?? "", tags).catch(reportError)}
    />
  );
}
