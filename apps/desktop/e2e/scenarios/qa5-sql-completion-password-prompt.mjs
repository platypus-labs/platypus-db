// BUG QA5-SQL-04 (medium). A connection whose password is asked at connect time (not saved, no
// password command) and connected from the navigator gets no table or column completion in the SQL
// editor opened afterwards: Ctrl+Space offers nothing at `select * from qa4`. The activity log never
// shows "Load the tables and columns of ... for completion". With a password command, or when the
// connection is made by running a statement in the editor (password prompt there), the catalog loads.
// Steps: PostgreSQL connection with the password not saved; expand it in the navigator, type the
// password in the prompt; double-click the connection (new SQL editor); type `select * from qa4`
// and press Ctrl+Space (schema public has qa4_g and qa4_u; any table does).
// Expected: the list offers the tables (qa4_g, qa4_u).
// Actual: an empty list; columns and `schema.` completion are missing too.
// Code: apps/desktop/src (the completion catalog is requested on connect; the connect that comes
// from the navigator through the password dialog does not request it, and the editor opened later
// finds a connected connection and does not either).
import { assert, connections, expand, needs, treeRow, visibleTab, CTRL } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  connections(dir, [["QA X", "pg"]]);
}

export default async function (page) {
  await expand(page, "QA X", "pg", "qa4_g");
  await page.eval(`document.querySelectorAll('#qa-conn').forEach(e => e.removeAttribute('id')); ${treeRow("QA X")}.id = 'qa-conn'`);
  await page.clickSel("#qa-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 30_000);
  await page.sleep(2000);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.type("select * from qa4");
  await page.sleep(300);
  await page.key(" ", "Space", 32, CTRL);
  await page.sleep(2000);
  const labels = await page.eval(`[...document.querySelectorAll('.cm-tooltip-autocomplete li')].map(l => l.textContent)`);
  assert.ok(labels.includes("qa4_g"), `completion offered ${JSON.stringify(labels)}`);
}
