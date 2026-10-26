// Object actions of the navigator: generated SQL, rename, drop, truncate and copying the
// qualified name. Statements come from the backend's Dialect (`generate_sql`).
import { createElement } from "react";
import { type ActionScopes, registerAction } from "../actions";
import {
  api,
  type DialectSupport,
  type DriverKind,
  type GenerateRequest,
  type GenerateTarget,
  type ObjectKind,
  toCommandError,
} from "../api";
import { catalogKey, renameCatalog } from "../catalog/catalog";
import { renamedObject } from "../catalog/renames";
import { isRelation } from "../components/Navigator";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { getSetting } from "../settings";
import { quoteIdent, selectLimit } from "../sqlNames";
import { workbench } from "../workbench";
import { SqlActionDialog, type SqlActionOptions } from "./SqlActionDialog";
import { t, msg } from "../i18n";

type ObjectArg = ActionScopes["navigator.object"];

const support = new Map<DriverKind, DialectSupport>();
for (const driver of ["postgres", "mysql", "sqlite", "duckdb", "sqlserver", "clickhouse", "libsql", "mongo"] as const) {
  api.dialectSupport(driver).then((s) => support.set(driver, s), console.warn);
}

const can = (driver: DriverKind, list: "droppable" | "renamable", kind: ObjectKind) =>
  support.get(driver)?.[list].includes(kind) ?? true;

const isTable = (kind: ObjectKind) => kind === "table" || kind === "foreign_table";

function target({ schema, object }: ObjectArg): GenerateTarget {
  return {
    schema: schema.current ? null : schema.name,
    name: object.name,
    kind: object.kind,
    parent: object.parent,
  };
}

/** The target with its columns and primary key, read from the navigator session. */
async function withColumns(arg: ObjectArg): Promise<GenerateTarget | null> {
  const sid = await workbench().sideSession(arg.connection, arg.database);
  if (sid === null) return null;
  const details = await api.tableDetails(sid, arg.schema.name, arg.object.name);
  return {
    ...target(arg),
    columns: details.columns.map((c) => c.name),
    keys: details.columns.filter((c) => c.primary_key).map((c) => c.name),
  };
}

/** Opens generated SQL in a new editor tab. */
async function generate(arg: ObjectArg, request: GenerateRequest, what: string) {
  const table = await withColumns(arg);
  if (!table) return;
  const sql = await api.generateSql(arg.connection.driver, table, request);
  if (sql === null) {
    workbench().showError(t("{what} needs a primary key on {name}.", { what, name: arg.object.name }));
    return;
  }
  workbench().openTab("query", arg.connection.id, {
    sql,
    title: `${arg.object.name} ${what}`,
    database: arg.database,
  });
}

const DML: { id: string; title: string; statement: GenerateRequest["statement"]; order: number }[] = [
  { id: "select", title: msg("Generate SELECT"), statement: "select", order: 20 },
  { id: "insert", title: msg("Generate INSERT"), statement: "insert", order: 21 },
  { id: "update", title: msg("Generate UPDATE"), statement: "update", order: 22 },
  { id: "delete", title: msg("Generate DELETE"), statement: "delete", order: 23 },
  { id: "upsert", title: msg("Generate UPSERT"), statement: "upsert", order: 24 },
];

for (const { id, title, statement, order } of DML) {
  registerAction({
    id: `object.generate_${id}`,
    scope: "navigator.object",
    title,
    group: "generate",
    order,
    // Writes go to tables; views get SELECT only.
    when: ({ object }) => (statement === "select" ? isRelation(object.kind) : isTable(object.kind)),
    run: (arg) =>
      generate(
        arg,
        statement === "select"
          ? { statement, limit: getSetting(selectLimit) }
          : ({ statement } as GenerateRequest),
        statement.toUpperCase(),
      ),
  });
}

registerAction({
  id: "object.generate_ddl",
  scope: "navigator.object",
  title: "Generate DDL",
  group: "generate",
  order: 29,
  multiple: true,
  run: async (arg) => {
    const sid = await workbench().sideSession(arg.connection, arg.database);
    if (sid === null) return;
    const objects = arg.selection ?? [arg];
    const ddl: string[] = [];
    // One at a time: they share the navigator's session.
    for (const { schema, object } of objects) {
      ddl.push(
        await api.objectDdl(sid, { schema: schema.name, name: object.name, kind: object.kind, parent: object.parent }),
      );
    }
    workbench().openTab("query", arg.connection.id, {
      sql: ddl.join("\n\n"),
      title: objects.length > 1 ? t("DDL of {n} objects", { n: objects.length }) : t("{name} DDL", { name: arg.object.name }),
      database: arg.database,
    });
  },
});

registerAction({
  id: "object.copy_qualified_name",
  scope: "navigator.object",
  title: "Copy qualified name",
  group: "copy",
  order: 41,
  multiple: true,
  run: (arg) => navigator.clipboard.writeText((arg.selection ?? [arg]).map(qualified).join("\n")),
});

/** `schema.name`, quoted; a PostgreSQL routine keeps its argument list unquoted. */
function qualified({ connection: { driver }, schema, object: { name } }: ObjectArg): string {
  const paren = driver === "postgres" ? name.indexOf("(") : -1;
  const quoted = paren >= 0 ? `${quoteIdent(name.slice(0, paren), driver)}${name.slice(paren)}` : quoteIdent(name, driver);
  return `${quoteIdent(schema.name, driver)}.${quoted}`;
}

/** Opens the confirmation dialog for a statement that changes an object. */
function confirmStatement(
  arg: ObjectArg,
  dialog: { title: string; description: string; confirm: string; options: SqlActionOptions },
  request: (choices: { cascade: boolean; newName: string }) => GenerateRequest,
  after?: (choices: { cascade: boolean; newName: string }) => void,
) {
  workbench().openDialog((close) =>
    createElement(SqlActionDialog, {
      connection: arg.connection,
      database: arg.database,
      ...dialog,
      generate: (choices) => api.generateSql(arg.connection.driver, target(arg), request(choices)),
      onDone: (choices) => {
        after?.(choices);
        requestNavigatorRefresh({
          connectionId: arg.connection.id,
          database: arg.database,
          schema: arg.schema.name,
        });
      },
      onClose: close,
    }),
  );
}

registerAction({
  id: "object.rename",
  scope: "navigator.object",
  title: "Rename…",
  group: "manage",
  order: 80,
  when: ({ connection, object }) => can(connection.driver, "renamable", object.kind),
  run: (arg) =>
    confirmStatement(
      arg,
      {
        title: t("Rename {name}", { name: arg.object.name }),
        description: t("Statements and views that use the old name stop working."),
        confirm: t("Rename"),
        options: { rename: arg.object.name.replace(/\(.*$/, "") },
      },
      ({ newName }) => ({ statement: "rename", new_name: newName }),
      ({ newName }) => {
        const from = catalogKey(arg.database, arg.schema.name, arg.object.name);
        const object = renamedObject(arg.object.name, newName);
        if (object) {
          renameCatalog(arg.connection.id, from, { ...from, object }).catch((e: unknown) =>
            workbench().showError(toCommandError(e).message),
          );
        }
      },
    ),
});

registerAction({
  id: "object.truncate",
  scope: "navigator.object",
  title: "Truncate…",
  group: "manage",
  order: 85,
  when: ({ object }) => object.kind === "table",
  run: (arg) =>
    confirmStatement(
      arg,
      {
        title: t("Truncate {name}", { name: arg.object.name }),
        description: t("Deletes every row of the table. This cannot be undone outside a transaction."),
        confirm: t("Truncate"),
        options: {},
      },
      () => ({ statement: "truncate" }),
    ),
});

registerAction({
  id: "object.drop",
  scope: "navigator.object",
  title: "Drop…",
  group: "manage",
  order: 90,
  when: ({ connection, object }) => can(connection.driver, "droppable", object.kind),
  run: (arg) =>
    confirmStatement(
      arg,
      {
        title: t("Drop {name}", { name: arg.object.name }),
        description: t("Removes the object and, for a table, all of its data."),
        confirm: t("Drop"),
        options: { cascade: support.get(arg.connection.driver)?.cascade ?? false },
      },
      ({ cascade }) => ({ statement: "drop", cascade }),
    ),
});
