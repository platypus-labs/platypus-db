// QA4 administration: Server activity opened on a connection that cannot connect (wrong
// password) shows an empty session list, "No sessions match." and counts of zero, as if the
// server had no sessions at all.
// Steps: save a PostgreSQL connection whose password command gives a wrong password, choose
//   Server activity from its menu in the navigator (the connection fails to open).
// Expected: the tab says it could not read the sessions and why (an error state, like the
//   other views of the tab do), not an empty list; no "No sessions match." and no zero counts.
// Actual: the filter chips read "0 all / 0 waiting / ...", the table says "No sessions match."
//   and the only sign of the failure is a toast that goes away.
// Code: apps/desktop/src/admin/AdminTab.tsx `load`: `if (sid === null) return;` leaves quietly
//   when the side session could not be opened, so neither `error` nor `snapshot` is set and the
//   tab renders the empty list.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DBS, needs, treeMenu } from "../qa-grid.mjs";
import { adminTab } from "../admin.mjs";

const DB = "pg";
const NAME = "QA4 admin wrong password";

export const skip = needs(DB);

export function setup({ dir }) {
  const toml = `${DBS[DB].toml}\npassword_command = "echo wrong-password"`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n\n[[connections]]\nid = "00000000-0000-4000-8000-0000000c0e01"\nname = "${NAME}"\n${toml}\nsave_password = false\n`,
  );
}

export default async function (page) {
  await treeMenu(page, NAME, "Server activity");
  await page.waitFor(`${adminTab} !== undefined`, 15_000);
  // Give the failed connection time to be reported (the toast) and the tab to settle.
  await page.sleep(6000);
  const text = await page.eval(`${adminTab}.innerText`);
  assert.ok(
    !text.includes("No sessions match."),
    `the tab shows an empty list of sessions instead of why they could not be read:\n${text}`,
  );
}
