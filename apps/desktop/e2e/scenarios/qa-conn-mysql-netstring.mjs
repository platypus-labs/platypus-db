// CONN-21 (low). A MySQL Connector/NET connection string, `Server=h;Port=3306;Database=app;
// Uid=root;Pwd=x;` (what .NET apps keep in their config), pasted in the Host field of the
// MySQL form, is read as SQL Server: the form offers to go back ("This URL is for SQL
// Server"). `Port=` is not a SqlClient key at all, and the form the user is in says MySQL.
// Steps: New connection, MySQL; paste the string into Host.
// Expected: the MySQL form filled (host h, port 3306, database app, user root).
// Actual: the "This URL is for SQL Server" notice; nothing filled.
// Suspected: crates/platypus-core/src/config/keyvalue.rs `read` (no `Driver=` means SQL
// Server) and ConnectionDialog `onPaste` (does not pass the form's driver as a hint).
import assert from "node:assert/strict";
import { formField, newConnection, paste, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "MySQL");
  await page.eval(paste(formField("Host"), "Server=h;Port=3306;Database=app;Uid=root;Pwd=x;"));
  await page.waitFor(`document.querySelector('.connection-dialog .url-other') !== null || ${formField("Host")}.value === 'h'`);
  assert.equal(await page.eval(`document.querySelector('.connection-dialog .url-other')?.textContent ?? null`), null);
  assert.equal(await page.eval(`${formField("User")}.value`), "root");
}
