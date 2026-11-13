// QA4-CONN-8 (low). Two connections can have the same name: saving a second "A" works and
// the navigator shows two identical rows (the command palette, the MCP server's and the CLI's
// connection names are ambiguous too). A network profile with a taken name is refused ("a
// network profile named … exists already"), and so is a saved script's.
// Steps: New connection, SQLite, name "QA lite" (the seeded connection's name); Save.
// Expected: refused with a message, or the name made unique ("QA lite (2)").
// Actual: a second row "QA lite".
// Code: crates/platypus-store/src/connections.rs `save` (checks the name only for network
// profiles, see `save_network_profile`).
import assert from "node:assert/strict";
import { button, dialog, fillForm, newConnection, saveForm, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, ctx) {
  await newConnection(page, "SQLite");
  await fillForm(page, { Name: "QA lite", "Database file": `${ctx.dir}/other.db` });
  await page.eval(`${button(dialog, "Save")}.click()`);
  await page.sleep(1000);
  if (await page.eval(`${dialog} !== null`)) return; // refused
  const names = await page.eval(`[...document.querySelectorAll('.tree-row .tree-label')].map(l => l.textContent)`);
  assert.equal(new Set(names).size, names.length, `two rows with the same name: ${names.join(", ")}`);
}
