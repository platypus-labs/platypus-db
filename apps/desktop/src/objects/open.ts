import { isNoSql, type DriverKind, type ObjectKind } from "../api";
import type { TabModel } from "../tabs/registry";
import { workbench } from "../workbench";

/** An object of a connection's database, as the tabs that show it name it. */
export interface ObjectPlace {
  database: string | null | undefined;
  schema: string;
  name: string;
}

const sameDatabase = (tab: TabModel, database: string | null | undefined) => (tab.database ?? null) === (database ?? null);

/** Whether the graphical table editor can alter the tables of `driver`: it writes SQL DDL. */
export const editsTables = (driver: DriverKind) => !isNoSql(driver);

/** Opens the graphical editor of a table, or brings forward the one already open on it. */
export function openTableEditor(connectionId: string, { database, schema, name }: ObjectPlace) {
  const showing = (t: TabModel) =>
    t.kind === "table-editor" && t.connectionId === connectionId && sameDatabase(t, database) && t.schema === schema && t.table === name;
  if (workbench().focusTab(showing)) return;
  workbench().openTab("table-editor", connectionId, { database, schema, table: name, title: name });
}

/** Opens the tab of an object (properties, columns, DDL…), or brings forward the one already open. */
export function openObjectTab(
  connectionId: string,
  object: ObjectPlace & { currentSchema: boolean; kind: ObjectKind; parent: string | null },
) {
  const { database, schema, name, kind, parent } = object;
  const showing = (t: TabModel) =>
    t.kind === "object" &&
    t.connectionId === connectionId &&
    sameDatabase(t, database) &&
    t.schema === schema &&
    t.name === name &&
    t.objectKind === kind &&
    (t.parent ?? null) === parent;
  if (workbench().focusTab(showing)) return;
  workbench().openTab("object", connectionId, {
    database,
    schema,
    currentSchema: object.currentSchema,
    name,
    objectKind: kind,
    parent,
    title: name,
  });
}
