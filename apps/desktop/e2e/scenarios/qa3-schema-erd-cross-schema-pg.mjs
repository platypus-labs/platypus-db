// SCH-08 (medium). The ER diagram matches a foreign key's table by name only: a key of
// qa3s_src.orders that references qa3s_dst.customers is drawn as a line to qa3s_src.customers,
// a different table that happens to have the same name, and the auto layout places orders next
// to it. The diagram shows a relationship that does not exist.
// Steps: qa3s_src.customers(id), qa3s_src.orders(customer_id → qa3s_dst.customers(id)),
// qa3s_dst.customers(id); navigator > qa3s_src > ER diagram.
// Expected: no line from orders to qa3s_src.customers (the referenced table is not in the
// diagram; at most a note of the other schema). Actual: one line orders → customers.
// Suspected: apps/desktop/src/erd/ErdTab.tsx:57 (box refs) and :260 (findIndex on
// fk.ref_table without fk.ref_schema).
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, needsEngines, openPath, rowMenu, saveEngines, visibleErd } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, DST, `CREATE TABLE S.customers (id int PRIMARY KEY);`);
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.customers (id int PRIMARY KEY, name text);
CREATE TABLE S.orders (id int PRIMARY KEY, customer_id int REFERENCES ${DST}.customers(id));`);
  saveEngines(dir, ["pg"]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA3 PG", "platypus", SRC]);
    await rowMenu(page, ["QA3 PG", "platypus"], SRC, "ER diagram");
    await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === 2`, 30_000);
    const lines = await page.eval(`[...${visibleErd}.querySelectorAll('path[marker-start]')].map(p => p.querySelector('title')?.textContent)`);
    assert.deepEqual(lines, [], "a line to a table of the same name in this schema");
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}
