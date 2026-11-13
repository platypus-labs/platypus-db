// JRN-06 (low). With the interface in Italian, the query manager (Ctrl+Maiusc+M, "Gestione
// delle query") lists the app's own metadata requests with English descriptions: "objects of
// main", "table details of main.orders", "DDL of main.orders", "properties of main.orders",
// "catalog columns", "schemas", "autocommit on".
// Steps: Italian; expand a connection and a table; open its details; Ctrl+Maiusc+M.
// Expected: the descriptions in Italian (they are the app's text, not SQL). Actual: English.
// Suspected: apps/desktop/src-tauri/src/querylog.rs:296-376 (`format!("objects of {schema}")`,
// "table details of …", "DDL of …", "properties of …"): English text shown as is.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL, SHIFT } from "../cdp.mjs";
import { italian, treeRow } from "../journey-helpers.mjs";
import { menu } from "../navigator-helpers.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "QA jrn manager";
const IT = italian();

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_jrn_manager.db",
    id: "00000000-0000-4000-8000-0000000a8002",
    name: NAME,
    sql: "CREATE TABLE orders (id INTEGER PRIMARY KEY, total REAL);",
  });
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("orders")} !== undefined`, 15_000);
  await menu(page, "orders", IT["Open details"]);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('orders')`, 15_000);
  await page.sleep(500);
  await page.key("M", "KeyM", 77, CTRL | SHIFT);
  await page.waitFor(`document.querySelectorAll('.query-manager tbody tr').length > 2`, 10_000);
  const texts = await page.eval(`[...document.querySelectorAll('.query-manager tbody tr .query-manager-text')].map(t => t.textContent)`);
  const english = texts.filter((t) => /^(objects of|table details of|DDL of|properties of|catalog columns|schemas$|autocommit (on|off))/.test(t));
  assert.deepEqual(english, [], "descriptions in English in the Italian interface");
}
