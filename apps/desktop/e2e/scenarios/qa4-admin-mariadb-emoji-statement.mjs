// QA4 administration, MariaDB: Server activity shows a statement with a character outside the
// Basic Multilingual Plane (an emoji) as four question marks.
// Steps: connect to MariaDB, start `SELECT 'qa4adm-emoji-😀', SLEEP(30)` from another session, open
//   Server activity, select that session.
// Expected: the statement (list and details) reads as it was sent.
// Actual: `SELECT 'qa4adm-emoji-????', SLEEP(30)`.
// Code: crates/platypus-driver-mysql/src/admin.rs, the sessions query reads
//   information_schema.PROCESSLIST.INFO (utf8mb3); MariaDB's INFO_BINARY holds the raw bytes.
import assert from "node:assert/strict";
import { connections, needs } from "../qa-grid.mjs";
import { adminTab, background, openAdmin, rowWith, select } from "../admin.mjs";

const DB = "mariadb";
const NAME = "QA4 admin MariaDB emoji";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  const holder = background(DB, "SELECT 'qa4adm-emoji-😀', SLEEP(30);\n");
  try {
    await openAdmin(page, NAME, DB);
    await page.waitFor(`${rowWith("qa4adm-emoji-")} !== undefined`, 30_000);
    await select(page, "qa4adm-emoji-");
    const shown = await page.eval(`${adminTab}.querySelector('.admin-statement').textContent`);
    assert.ok(shown.includes("qa4adm-emoji-😀"), `the statement shows as ${JSON.stringify(shown)}`);
  } finally {
    holder.child.kill();
  }
}

