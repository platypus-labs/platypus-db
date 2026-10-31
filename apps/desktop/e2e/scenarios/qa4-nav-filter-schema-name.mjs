// qa4-nav-2 (medium). Typing a schema's name in the navigator's filter hides that schema
// (and everything else): the filter matches object names only.
// Steps: PostgreSQL with schemas qa4_nav_fa (tables users, orders) and qa4_nav_fb; open both
//   schemas; type `qa4_nav_fa` in "Filter objects…".
// Expected: the schema qa4_nav_fa stays, with its objects (or at least the schema row), as the
//   filter of any tree does when it matches a folder's name; at least not an empty tree.
// Actual: the schema row disappears: only unrelated, not yet loaded schemas remain.
//   Likewise `qa4_nav_fa.users` (the qualified name the app copies) finds nothing.
// Code: apps/desktop/src/components/Navigator.tsx schemaRows(): `matches` filters on
//   `o.name` only and `if (needle && objects && matches.length === 0) continue` drops the schema.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, needs, row, saveConnections, treeLabels } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS qa4_nav_fa CASCADE; DROP SCHEMA IF EXISTS qa4_nav_fb CASCADE;
    CREATE SCHEMA qa4_nav_fa; CREATE SCHEMA qa4_nav_fb;
    CREATE TABLE qa4_nav_fa.users (id int); CREATE TABLE qa4_nav_fa.orders (id int);
    CREATE TABLE qa4_nav_fb.stuff (id int);`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a4002"));
}

const filter = async (page, text) => {
  await page.eval(`document.querySelector('.tree-filter').focus()`);
  await page.key("a", "KeyA", 65, 2);
  await page.type(text);
  await page.sleep(700);
  return page.eval(treeLabels);
};

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, "qa4_nav_fa", "users");
    await expand(page, "qa4_nav_fb", "stuff");
    const byName = await filter(page, "qa4_nav_fa");
    assert.ok(byName.includes("qa4_nav_fa"), `schema name filter: ${JSON.stringify(byName)}`);
    const qualified = await filter(page, "qa4_nav_fa.users");
    assert.ok(qualified.includes("users"), `qualified name filter: ${JSON.stringify(qualified)}`);
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS qa4_nav_fa CASCADE; DROP SCHEMA IF EXISTS qa4_nav_fb CASCADE;`);
  }
}
