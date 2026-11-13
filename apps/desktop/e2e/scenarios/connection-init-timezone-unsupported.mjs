// CONN-07 (medium). The Init tab offers "Time zone" for SQL Server and MongoDB, but neither
// has a session time zone: with any value (Europe/Rome too) the connection fails with
// "SQL Server has no session time zone" / "MongoDB has no session time zone". The form lets
// the user break the connection with a field that cannot work.
// Steps: New connection, SQL Server (then MongoDB), Init tab.
// Expected: no Time zone field for drivers without a session time zone (as libSQL and the
// embedded drivers). Actual: the field is there.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx (Init page shows the time zone
// for every server driver but libSQL).
import assert from "node:assert/strict";
import { button, dialog, newConnection, picker, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  const offered = [];
  for (const tile of ["SQL Server", "MongoDB"]) {
    await newConnection(page, tile);
    await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Init')).click()`);
    await page.waitFor(`document.querySelector('[data-page="init"]').hidden === false`);
    if (await page.eval(`document.querySelector('[data-field="init_time_zone"]') !== null`)) offered.push(tile);
    await page.eval(`${button(dialog, "Cancel")}.click()`);
    await page.waitFor(`${dialog} === null && ${picker} === null`);
  }
  assert.deepEqual(offered, [], `Time zone offered for: ${offered.join(", ")}`);
}
