// QA4 administration (passes): the server ends the session Server activity reads on (a DBA
// or a timeout kills it); the tab keeps refreshing, either on a new session or with the
// error shown, and never stays on the old list as if nothing happened.
import assert from "node:assert/strict";
import { connections, needs, sh } from "../grid-helpers.mjs";
import { adminTab, openAdmin, setRefresh } from "../admin.mjs";

const DB = "pg";
const NAME = "QA4 admin killed side session";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

const mine = () => sh(DB, "SELECT pid FROM pg_stat_activity WHERE application_name = 'PlatypusDB' AND pid <> pg_backend_pid() ORDER BY backend_start");

export default async function (page) {
  await openAdmin(page, NAME, DB);
  await page.waitFor(`${adminTab}.querySelector('tr.admin-session.current') !== null`, 20_000);
  const ownId = await page.eval(`${adminTab}.querySelector('tr.admin-session.current').dataset.id`);
  assert.ok(mine().split("\n").includes(ownId), `the app's session ${ownId} is among ${mine()}`);
  sh(DB, `SELECT pg_terminate_backend(${ownId})`);
  await setRefresh(page, 2);
  // Within a few readings the list is again the server's: the app's own session is listed (a
  // new one), or the tab says why it cannot read.
  await page.waitFor(
    `(${adminTab}.querySelector('tr.admin-session.current')?.dataset.id ?? '${ownId}') !== '${ownId}' || ${adminTab}.querySelector('.notice.error, .admin-error, [data-field="admin-error"]') !== null`,
    40_000,
  );
}
