// Transfer profiles of exports (group H) on SQLite: a query export and a tables export saved
// from their dialogs, run from the profiles dialog (the files are checked), opened again with
// what they kept, "Save profile" replacing a profile and "Save as new profile" copying it, a
// name already taken refused (on save and on rename), rename, delete asked first, the list of
// a connection's menu holding only its own profiles, and a table dropped since the save left
// out of the tables export with a notice (and the run of the stale profile failing).
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import {
  cancelName,
  closeProfiles,
  deleteProfile,
  listed,
  nameIt,
  openProfiles,
  openProfilesOf,
  profileRow,
  profilesDialog,
  rowAction,
  runProfile,
  saveProfile,
  savedProfiles,
} from "../profiles-helpers.mjs";
import { answerSave, button, closeModal, exportTablesDialog, expand, fill, menu, openEditor, runScript, sqliteBlock, saveConnections, visibleTab } from "../transfer-helpers.mjs";

const SHOP = "Profiles shop";
const OTHER = "Profiles other";
const exportDialog = (title) => `([...document.querySelectorAll('.modal')].find(m => m.querySelector('h2')?.textContent === ${JSON.stringify(title)}) ?? null)`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "shop.db"));
  db.exec(`
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, qty INTEGER);
    CREATE TABLE extra (id INTEGER PRIMARY KEY);
    INSERT INTO customers (name) VALUES ('Ada'), ('Alan');
    INSERT INTO orders (customer_id, qty) VALUES (1, 3), (2, 10);
    INSERT INTO extra (id) VALUES (1);
  `);
  db.close();
  new DatabaseSync(join(dir, "other.db")).close();
  saveConnections(
    dir,
    [],
    sqliteBlock(SHOP, join(dir, "shop.db"), "00000000-0000-4000-8000-0000000000a1") +
      sqliteBlock(OTHER, join(dir, "other.db"), "00000000-0000-4000-8000-0000000000a2"),
  );
}

export default async function (page, { dir }) {
  const out = join(dir, "out");
  const tables = join(out, "tables");
  mkdirSync(tables, { recursive: true });
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(tables)})`);

  // A query export, as JSON, saved from the export dialog of a result.
  await openEditor(page, SHOP);
  assert.match(await runScript(page, "SELECT id, name FROM customers ORDER BY id"), /2 rows/);
  await page.eval(`[...${visibleTab}.querySelectorAll('button.link')].find(b => b.textContent === 'Export…').click()`);
  const exporting = exportDialog("Export result");
  await page.waitFor(`${exporting} !== null`);
  await page.eval(fill(`${exporting}.querySelector('select')`, "json"));
  await page.waitFor(`${exporting}.querySelector('select').value === 'json'`);
  await page.eval(`${exporting}.querySelectorAll('fieldset.choices input')[0].click()`);
  // Not a profile yet: there is nothing to replace.
  assert.equal(await page.eval(`${button(exporting, "Save profile")} === undefined`), true);
  await answerSave(page, join(out, "customers.json"));
  assert.equal(await saveProfile(page, exporting, "Save as profile…", "Customers JSON"), null);
  await closeModal(page, exporting);
  const kept = savedProfiles(dir)["Customers JSON"];
  assert.equal(kept.task.kind, "export_query");
  assert.equal(kept.task.sql, "SELECT id, name FROM customers ORDER BY id");
  assert.equal(kept.task.options.format, "json");
  assert.ok(kept.task.path.endsWith("customers.json"));

  // Every table of the schema, as CSV with semicolons, into a folder.
  await expand(page, SHOP, "main");
  await menu(page, "main", "Export all tables", exportTablesDialog, SHOP);
  await page.waitFor(`['customers', 'extra', 'orders'].every(n => ${exportTablesDialog}.textContent.includes(n))`, 15_000);
  await page.eval(fill(`[...${exportTablesDialog}.querySelectorAll('label.field')].find(l => l.textContent.startsWith('Delimiter')).querySelector('select')`, ";"));
  await page.waitFor(`[...${exportTablesDialog}.querySelectorAll('label.field')].find(l => l.textContent.startsWith('Delimiter')).querySelector('select').value === ';'`);
  assert.equal(await saveProfile(page, exportTablesDialog, "Save as profile…", "All shop tables"), null);
  await closeModal(page, exportTablesDialog);
  const keptTables = savedProfiles(dir)["All shop tables"];
  assert.equal(keptTables.task.kind, "export_tables");
  assert.deepEqual(keptTables.task.tables.map((t) => t.name).sort(), ["customers", "extra", "orders"]);
  assert.equal(keptTables.task.options.delimiter, ";");

  // The list: both, by name, with what each does and where.
  await openProfiles(page);
  assert.deepEqual(await listed(page), ["All shop tables", "Customers JSON"]);
  assert.match(await page.eval(`${profileRow("Customers JSON")}.textContent`), /Export a query · Profiles shop/);
  assert.match(await page.eval(`${profileRow("All shop tables")}.textContent`), /Export tables · Profiles shop/);
  await closeProfiles(page);

  // Run from the list: the files are the dialogs' own.
  let run = await runProfile(page, "Customers JSON");
  assert.equal(run.error, null);
  assert.match(run.text, /Done: 2 rows\./);
  assert.deepEqual(JSON.parse(readFileSync(join(out, "customers.json"), "utf8")), [
    { id: 1, name: "Ada" },
    { id: 2, name: "Alan" },
  ]);
  run = await runProfile(page, "All shop tables");
  assert.equal(run.error, null);
  assert.match(run.text, /Done: [0-9]+ rows\./);
  assert.deepEqual(readdirSync(tables).sort(), ["customers.csv", "extra.csv", "orders.csv"]);
  assert.equal(readFileSync(join(tables, "customers.csv"), "utf8").replaceAll("\r\n", "\n"), "id;name\n1;Ada\n2;Alan\n");

  // Open: the dialog starts with what the profile kept. "Save profile" replaces it.
  await openProfiles(page);
  await rowAction(page, "Customers JSON", "Open…");
  const opened = exportDialog("Customers JSON");
  await page.waitFor(`${opened} !== null`, 15_000);
  assert.equal(await page.eval(`${opened}.querySelector('select').value`), "json");
  assert.equal(await page.eval(`${button(opened, "Save profile")} !== undefined`), true);
  await page.eval(fill(`${opened}.querySelector('select')`, "csv"));
  await page.waitFor(`${opened}.querySelector('select').value === 'csv'`);
  await answerSave(page, join(out, "customers2.csv"));
  await page.eval(`${button(opened, "Save profile")}.click()`);
  await page.waitFor(`${opened}.textContent.includes('Saved as profile Customers JSON.')`, 15_000);
  let all = savedProfiles(dir);
  assert.deepEqual(Object.keys(all).sort(), ["All shop tables", "Customers JSON"], "replaced, not added");
  assert.equal(all["Customers JSON"].task.options.format, "csv");
  assert.ok(all["Customers JSON"].task.path.endsWith("customers2.csv"));

  // "Save as new profile": a copy next to it; the original keeps its choices.
  await answerSave(page, join(out, "customers3.csv"));
  assert.equal(await saveProfile(page, opened, "Save as new profile…", "Customers copy"), null);
  all = savedProfiles(dir);
  assert.deepEqual(Object.keys(all).sort(), ["All shop tables", "Customers JSON", "Customers copy"]);
  assert.notEqual(all["Customers copy"].id, all["Customers JSON"].id);
  assert.ok(all["Customers copy"].task.path.endsWith("customers3.csv"));
  assert.ok(all["Customers JSON"].task.path.endsWith("customers2.csv"));

  // A name in use, in any case, is refused and nothing is added.
  await answerSave(page, join(out, "customers4.csv"));
  assert.match(await saveProfile(page, opened, "Save as new profile…", "CUSTOMERS COPY"), /profile already/);
  await cancelName(page);
  assert.equal(Object.keys(savedProfiles(dir)).length, 3);
  await closeModal(page, opened);

  // Rename: kept; a name in use is refused.
  await openProfiles(page);
  await rowAction(page, "Customers copy", "Rename…");
  await page.waitFor(`document.querySelector('.modal.name-dialog input')?.value === 'Customers copy'`);
  assert.match((await nameIt(page, "all SHOP tables")) ?? "", /profile already/);
  await cancelName(page);
  await rowAction(page, "Customers copy", "Rename…");
  await page.waitFor(`document.querySelector('.modal.name-dialog input')?.value === 'Customers copy'`);
  assert.equal(await nameIt(page, "Customers renamed"), null);
  await page.waitFor(`${profileRow("Customers renamed")} !== undefined && ${profileRow("Customers copy")} === undefined`);
  assert.deepEqual(Object.keys(savedProfiles(dir)).sort(), ["All shop tables", "Customers JSON", "Customers renamed"]);
  await closeProfiles(page);

  // A connection's menu lists its profiles only: none for the other, all three for the shop.
  await openProfilesOf(page, OTHER);
  assert.match(await page.eval(`${profilesDialog}.querySelector('h2').textContent`), /Transfer profiles of Profiles other/);
  assert.deepEqual(await listed(page), []);
  assert.match(await page.eval(`${profilesDialog}.textContent`), /No profiles saved yet\./);
  await closeProfiles(page);
  await openProfilesOf(page, SHOP);
  assert.deepEqual(await listed(page), ["All shop tables", "Customers JSON", "Customers renamed"]);
  await closeProfiles(page);

  // Delete asks first and removes the profile from the file too.
  await openProfiles(page);
  assert.equal(await deleteProfile(page, "Customers renamed"), null);
  assert.deepEqual(await listed(page), ["All shop tables", "Customers JSON"]);
  assert.deepEqual(Object.keys(savedProfiles(dir)).sort(), ["All shop tables", "Customers JSON"]);
  await closeProfiles(page);

  // A table dropped since the save: the dialog says so and exports the others.
  const shop = new DatabaseSync(join(dir, "shop.db"));
  shop.exec("DROP TABLE extra");
  shop.close();
  run = await runProfile(page, "All shop tables");
  assert.ok(run.error !== null, `the stale profile ran: ${run.text}`);
  await openProfiles(page);
  await rowAction(page, "All shop tables", "Open…");
  const tablesDialog = exportTablesDialog;
  await page.waitFor(`${tablesDialog} !== null && ${tablesDialog}.textContent.includes('No longer in the database, left out: extra.')`, 20_000);
  assert.ok(!(await page.eval(`${tablesDialog}.textContent`)).includes("extra.csv"));
  for (const file of readdirSync(tables)) rmSync(join(tables, file));
  await page.waitFor(`!${button(tablesDialog, "Export to folder…")}.disabled`);
  await page.eval(`${button(tablesDialog, "Export to folder…")}.click()`);
  await page.waitFor(`${tablesDialog}.textContent.includes('Exported')`, 30_000);
  assert.deepEqual(readdirSync(tables).sort(), ["customers.csv", "orders.csv"]);
  assert.equal(existsSync(join(tables, "extra.csv")), false);
  await closeModal(page, tablesDialog);
}
