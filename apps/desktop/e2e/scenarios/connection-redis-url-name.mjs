// QA5-CONN-5 (low). Pasting a Redis URL that names a database number
// (`redis://:secret@cache.example.com:6380/3`, the usual REDIS_URL) fills the connection's name
// with "3": the database index is taken for the database's name, as for a SQL database.
// A navigator row called "3" says nothing about the server.
// Steps: New connection, Redis; paste `redis://:pw@cache.example.com:6380/3` in Name.
// Expected: a name from the host ("cache.example.com"). Actual: "3".
// Code: apps/desktop/src/connectionUrl.ts `withParsed` (`named` = the database without its
//   options, whatever the driver; Redis' "database" is a number).
import assert from "node:assert/strict";
import { formField, newConnection, paste, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "Redis");
  await page.eval(paste(formField("Name"), "redis://:pw@cache.example.com:6380/3"));
  await page.waitFor(`${formField("Host")}.value === 'cache.example.com'`, 5000);
  const name = await page.eval(`${formField("Name")}.value`);
  assert.notEqual(name, "3", "the connection is named after the database number");
}
