// QA5-CONN-6 (low). The driver picker's search finds a database by its label or its driver id
// only, so the names people actually type find nothing: "mssql" (SQL Server's common
// name), "pg" (PostgreSQL) and "valkey" (a Redis-protocol server) give an empty grid, while
// "sqlserver" and "postgres" work. Enter on the empty result does nothing.
// Steps: New connection; type "mssql" in the search (then "pg").
// Expected: SQL Server (PostgreSQL) among the tiles. Actual: no tile.
// Code: apps/desktop/src/components/DriverPicker.tsx (filter: `fold(label).includes(needle) ||
//   c.driver.includes(needle)`, no aliases).
import assert from "node:assert/strict";
import { openPicker, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await openPicker(page);
  await page.waitFor(`document.querySelectorAll('.driver-tile').length > 10`, 15_000);
  const failures = [];
  for (const [term, expected] of [["mssql", "SQL Server"], ["pg", "PostgreSQL"], ["valkey", "Redis"]]) {
    await page.eval(`(() => { const i = document.querySelector('.modal.driver-picker input'); i.focus();
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(i, ${JSON.stringify(term)});
      i.dispatchEvent(new Event('input', { bubbles: true })); })()`);
    await page.sleep(200);
    const tiles = await page.eval(`[...document.querySelectorAll('.driver-tile')].map(t => t.textContent)`);
    if (!tiles.includes(expected)) failures.push(`"${term}" finds [${tiles.join(", ")}], not ${expected}`);
  }
  assert.deepEqual(failures, [], failures.join(" | "));
}
