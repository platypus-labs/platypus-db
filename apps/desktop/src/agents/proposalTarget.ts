// What a proposal is about, for the messages that say it could not be opened. Everything here
// comes from an agent, so it is only ever shown as text.
// No app imports: the unit tests load it as it is.

/** The table a proposal of rows to change names (`schema.table`), read from its JSON without
 *  trusting it; `null` when it does not say. Long names are cut. */
export function proposedTable(edits: string): string | null {
  try {
    const parsed: unknown = JSON.parse(edits);
    if (typeof parsed !== "object" || parsed === null) return null;
    const { schema, table } = parsed as { schema?: unknown; table?: unknown };
    if (typeof table !== "string" || !table) return null;
    const name = typeof schema === "string" && schema ? `${schema}.${table}` : table;
    return name.length > 80 ? `${name.slice(0, 80)}…` : name;
  } catch {
    return null;
  }
}
