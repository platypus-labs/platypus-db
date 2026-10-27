// XFER-29 (medium). "Export data…" on a MongoDB collection fails.
// Steps: navigator › a MongoDB collection (the seed's customers) › "Export data…" (or the
// database's "Export all tables…"), CSV, choose a folder.
// Expected: customers.csv with the documents, as the result grid's own Export… writes them.
// Actual: "unknown statement `SELECT`: expected `db.`, `show` or `use`" and no file: the
// export builds `SELECT * FROM customers` for every driver.
// Suspected: apps/desktop/src-tauri/src/commands/export.rs:104-112 (SQL for the source
// whatever the driver) and apps/desktop/src/transfer/actions.ts:84-104 (offered on MongoDB).
// Needs MongoDB from docker/compose.yml (its seed's customers collection).
import assert from "node:assert/strict";
import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { closeModal, DBS, exportTablesDialog, menu, runJob, saveConnections, showTable, skipUnless } from "../qa-xfer.mjs";

export const skip = () => skipUnless("mongo");
export const setup = ({ dir }) => saveConnections(dir, ["mongo"]);

export default async function (page, { dir }) {
  const out = join(dir, "out");
  mkdirSync(out);
  await showTable(page, DBS.mongo.name, "platypus", "customers");
  await page.eval(`window.__platypus.answer('folder', ${JSON.stringify(out)})`);
  await menu(page, "customers", "Export data", exportTablesDialog, [DBS.mongo.name, "platypus"]);
  const result = await runJob(page, exportTablesDialog, "Export to folder…");
  await closeModal(page, exportTablesDialog);
  assert.match(result, /^Exported/);
  assert.ok(existsSync(join(out, "customers.csv")));
}
