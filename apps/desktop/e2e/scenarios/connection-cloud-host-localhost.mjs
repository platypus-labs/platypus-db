// CONN-12 (low). The Snowflake form's "Account" and the Databricks form's "Server hostname"
// start filled with `localhost` (the default host of every connection), hiding their
// placeholders (`xy12345.eu-west-1`, `dbc-1234.cloud.databricks.com`); neither service can
// be on localhost, so the user has to delete it first.
// Steps: New connection, Snowflake (then Databricks).
// Expected: the field empty, its placeholder visible. Actual: `localhost`.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx / DriverPicker presets (the
// tile keeps SavedConnection::new's host).
import assert from "node:assert/strict";
import { button, dialog, formField, newConnection, picker, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  const filled = {};
  for (const [tile, label] of [["Snowflake", "Account"], ["Databricks", "Server hostname"]]) {
    await newConnection(page, tile);
    filled[tile] = await page.eval(`${formField(label)}.value`);
    await page.eval(`${button(dialog, "Cancel")}.click()`);
    await page.waitFor(`${dialog} === null && ${picker} === null`);
  }
  assert.deepEqual(filled, { Snowflake: "", Databricks: "" });
}
