// What the schema QA4 scenarios (scenarios/qa4-schema-*.mjs) share: a schema per engine seeded from
// the shell (the qa3-schema.mjs engines), and the table editor's backend called straight:
// table_design reads a table as the form shows it, table_script makes the script of an edit,
// apply_table_script runs it. What the form shows is the design object; the SQL it writes is
// `script.sql`, what the database then holds is read back with the engine's own client.
import { ENGINES, SRC, connectionOf, invoke, saveEngines, session } from "./qa3-schema.mjs";

export const Q4 = "qa3s_q4";
export const DRIVER = { sqlite: "sqlite", pg: "postgres", mysql: "mysql", mariadb: "mysql", mssql: "sqlserver" };

/** Seeds `sql` (`S.` standing for the schema) in `Q4` of `key`. */
export function seed(dir, key, sql) {
  ENGINES[key].seed(dir, key === "sqlite" ? SRC : Q4, sql);
}

/** A design handle on the schema `Q4` of `key`. */
export async function lab(page, dir, key) {
  const connection = connectionOf(dir, key, key === "sqlite" ? SRC : Q4);
  const sid = await session(page, connection.id);
  const schema = ENGINES[key].schema(Q4);
  const driver = DRIVER[key];
  const call = async (cmd, args) => {
    const r = await invoke(page, cmd, args);
    if (r.err) throw new Error(`${cmd}: ${r.err}`);
    return r.ok;
  };
  return {
    sid,
    schema,
    design: (table) => call("table_design", { sessionId: sid, schema, table }),
    script: (original, design) => call("table_script", { driver, schema, original, design }),
    /** Runs the script an edit makes; resolves to `{ sql, error }`. */
    async edit(table, change) {
      const original = await this.design(table);
      const design = structuredClone(original);
      change(design);
      const made = await this.script(original, design);
      const r = await invoke(page, "apply_table_script", { sessionId: sid, script: made.script });
      return { sql: made.sql, warnings: made.script.warnings, error: r.err ?? null };
    },
  };
}

export { ENGINES, saveEngines };


