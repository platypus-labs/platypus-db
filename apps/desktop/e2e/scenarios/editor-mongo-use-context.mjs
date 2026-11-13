// SQL-12. `use <db>` in the MongoDB editor switches the session's database, but the tab's
// chip keeps showing the old one: the next `db.x.insertOne(...)` writes into the other
// database while the tab says "platypus". The same holds for SQL: MySQL `USE other`,
// SQL Server `USE master` and PostgreSQL `SET search_path` leave the chip (and completion)
// on the old database/schema.
// Steps: Mongo editor, run `use admin`, look at the chip next to the toolbar.
// Expected: the chip shows admin (or `use` is refused/handled by switching the tab's database).
// Actual: the chip still shows platypus.
// Code: apps/desktop/src/components/QueryTab.tsx ConnectionChip shows tab.database; nothing
// updates it after `use` (driver-mongo shell) or USE / search_path.
import assert from "node:assert/strict";
import { DB, openEditor, runStatement, saveConnections, skipUnless, visibleTab } from "../editor-helpers.mjs";

export const skip = () => skipUnless("mongo");
export const setup = ({ dir }) => saveConnections(dir, ["mongo"]);

const chip = `(${visibleTab}.querySelector('.conn-chip')?.textContent ?? '')`;

export default async function (page) {
  await openEditor(page, DB.mongo.name);
  await runStatement(page, "db.getCollectionNames()");
  assert.match(await page.eval(chip), /platypus/);
  await runStatement(page, "use admin");
  await page.sleep(500);
  const shown = await page.eval(chip);
  await runStatement(page, "use platypus");
  assert.match(shown, /admin/, `after "use admin" the tab still shows: ${shown}`);
}
