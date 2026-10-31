// What the QA5 scenarios on the DDL the navigator generates for PostgreSQL tables share: a
// schema built with `sql`, then "Generate DDL" on `table` and the text of the editor tab.
import { DBS, connectTo, editorText, expand, menu, needs, saveConnections } from "./qa-nav-helpers.mjs";

/** The `skip`, `setup` and steps of a scenario that creates `schema` with `sql` and hands the DDL
 *  of `table` to `check(ddl)`. */
export function pgDdl({ schema, sql, table, id, check }) {
  const drop = `DROP SCHEMA IF EXISTS ${schema} CASCADE;`;
  return {
    skip: needs("postgres"),
    setup({ dir }) {
      DBS.postgres.exec(`${drop} CREATE SCHEMA ${schema}; ${sql}`);
      saveConnections(dir, DBS.postgres.connection("QA PG", `00000000-0000-4000-8000-0000000a5c${id}`));
    },
    async run(page) {
      try {
        await connectTo(page, "QA PG", "platypus");
        await expand(page, schema, table);
        await menu(page, table, "Generate DDL");
        await page.waitFor(`/CREATE/.test(${editorText})`, 15_000);
        check(await page.eval(editorText));
      } finally {
        DBS.postgres.exec(drop);
      }
    },
  };
}
