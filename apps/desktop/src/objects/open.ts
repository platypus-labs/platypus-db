import { api, isNoSql, type DriverKind, type ObjectKind, type SavedConnection } from "../api";
import { requestDataAgain } from "../navigatorEvents";
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

/** Opens the tab of an object (properties, columns, DDL…), or brings forward the one already
 *  open. A `parent` left out (the data viewer does not know a partition's table) matches any. */
export function openObjectTab(
  connectionId: string,
  object: ObjectPlace & { currentSchema: boolean; kind: ObjectKind; parent?: string | null },
) {
  const { database, schema, name, kind, parent } = object;
  const showing = (t: TabModel) =>
    t.kind === "object" &&
    t.connectionId === connectionId &&
    sameDatabase(t, database) &&
    t.schema === schema &&
    t.name === name &&
    t.objectKind === kind &&
    (parent === undefined || (t.parent ?? null) === parent);
  if (workbench().focusTab(showing)) return;
  workbench().openTab("object", connectionId, {
    database,
    schema,
    currentSchema: object.currentSchema,
    name,
    objectKind: kind,
    parent: parent ?? null,
    title: name,
  });
}

/** Whether `tab` is the data viewer of `table` (its qualified name) on the connection. */
export const showsData = (tab: TabModel, connectionId: string, database: string | null | undefined, table: string) =>
  tab.kind === "query" && tab.connectionId === connectionId && sameDatabase(tab, database) && tab.table === table;

/** The hidden key (CockroachDB's `rowid`) a table is edited by: `SELECT *` leaves it out, so the
 *  data viewer must select it. `null` when there is none or it cannot be read. Every way of
 *  opening a table viewer asks here, so they all open it editable. */
export async function hiddenKeyOf(
  connection: SavedConnection,
  database: string | null | undefined,
  schema: string,
  table: string,
): Promise<string | null> {
  if (connection.driver !== "postgres") return null; // Only CockroachDB (a postgres driver) has one.
  try {
    const id = await workbench().sideSession(connection, database ?? null);
    if (id === null) return null;
    return (await api.listObjects(id, schema)).find((o) => o.name === table)?.hidden_key ?? null;
  } catch {
    return null; // Opened without it; the editor checks the keys again.
  }
}

/** Brings forward the data viewer of `table`, which reads its rows again if its last read
 *  failed (the server was down, say); `false` when no tab shows it. */
export function focusData(connectionId: string, database: string | null | undefined, table: string): boolean {
  let key: number | undefined;
  const shown = workbench().focusTab((t) => {
    if (key === undefined && showsData(t, connectionId, database, table)) key = t.key;
    return t.key === key;
  });
  if (shown && key !== undefined) requestDataAgain(key);
  return shown;
}