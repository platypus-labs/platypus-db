// Bug NAV: on MongoDB the navigator offers the SQL table editor. "Edit table…" on a collection
// opens a tab that only shows `unknown statement \`SELECT\`: expected \`db.\`, \`show\` or
// \`use\``, and "Create table…" on a database previews `CREATE TABLE platypus.… (id INTEGER
// NOT NULL, …)`, SQL that MongoDB cannot run. The other SQL-only entries (Visual query
// builder, Search data…) are already hidden for MongoDB.
//
// Cause: the `table_editor.create` / `table_editor.edit` actions in
// apps/desktop/src/tabs/tableEditor.ts have no `when` excluding the mongo driver.
//
// Steps: expand the MongoDB connection; right-click the collection customers, then the
// database platypus.
// Expected: neither menu offers the table editor. Actual: both do, and it fails.
import assert from "node:assert/strict";
import { DBS, connectTo, needs, openMenu, row, saveConnections } from "../navigator-helpers.mjs";

export const skip = needs("mongo");

export function setup({ dir }) {
  saveConnections(dir, DBS.mongo.connection("QA Mongo", "00000000-0000-4000-8000-0000000a0051"));
}

export default async function (page) {
  await connectTo(page, "QA Mongo", "platypus");
  await page.waitFor(`${row("customers")} !== undefined`, 20_000);
  const collection = await openMenu(page, "customers");
  await page.key("Escape", "Escape", 27);
  const database = await openMenu(page, "platypus");
  await page.key("Escape", "Escape", 27);
  assert.ok(!collection.includes("Edit table…"), `collection menu: ${collection.join(", ")}`);
  assert.ok(!database.includes("Create table…"), `database menu: ${database.join(", ")}`);
}
