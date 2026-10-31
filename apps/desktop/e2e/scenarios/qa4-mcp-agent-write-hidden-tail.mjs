// qa4-mcp-4 (medium, confirmation spoofing). The "agent wants to change data" dialog shows the statement
// in a box of 260 px that scrolls: an agent (or a prompt-injected one) pads the statement with blank
// lines (or spaces) so the part that matters sits below the fold. The user sees
// `UPDATE orders SET status = 'x' WHERE id = 1` and a box that looks finished, the only hint being a thin
// scroll bar, while the statement ends `OR 1 = 1` and changes every row. On SQLite (and any driver with no
// row estimate) "The database gives no estimate of the rows it changes" is all the dialog offers.
// Steps: confirm_writes on; agent runs `UPDATE orders SET status = 'x' WHERE id = 1` + 60 blank lines + `OR 1 = 1`.
// Expected: the dialog shows the whole statement without scrolling for the user to miss it (blank runs
//   collapsed, or a clear "N more lines hidden" marker / the tail first), so what is allowed is what is seen.
// Actual: the box is 258 px of a 1077 px statement; the visible part ends at `WHERE id = 1`.
// Code: apps/desktop/src/ (the agent-write dialog, `.agent-write [data-field=statement]`, max-height 260px, overflow auto).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { noMcp, startMcp } from "../mcp.mjs";

export const skip = noMcp;

export function setup({ dir }) {
  const path = join(dir, "tail.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE orders (id INTEGER PRIMARY KEY, status TEXT); INSERT INTO orders VALUES (1,'a'),(2,'b'),(3,'c');");
  db.close();
  writeFileSync(join(dir, "connections.toml"), `version = 4\n[[connections]]\nid = "00000000-0000-4000-8000-000000000462"\nname = "Scratch"\ndriver = "sqlite"\ndatabase = '${path}'\nagent_access = "read_write"\n`);
  writeFileSync(join(dir, "settings.toml"), "[agents]\nconfirm_writes = true\n");
}

const dialog = `document.querySelector('.agent-write')`;

export default async function (page, { dir }) {
  const mcp = await startMcp(dir, "qa4-tail");
  try {
    const sql = `UPDATE orders SET status = 'x' WHERE id = 1${"\n".repeat(60)}OR 1 = 1`;
    const result = mcp.call("run_query", { connection: "Scratch", sql });
    await page.waitFor(`${dialog} !== null`, 20_000);
    const [scroll, client] = await page.eval(`(() => { const e = ${dialog}.querySelector('[data-field=statement]'); return [e.scrollHeight, e.clientHeight]; })()`);
    await page.eval(`${dialog}.querySelector('[data-answer=deny]').click()`);
    await result;
    assert.ok(scroll <= client + 1, `the statement box hides its tail (${scroll}px of content in ${client}px): \`OR 1 = 1\` needs a scroll to be seen`);
  } finally {
    mcp.close();
  }
}
