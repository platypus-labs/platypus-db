// CONN-06 (low). The password field of an existing connection says "•••••• (saved)" whenever
// the source is "Saved in the keychain", whether or not a password is saved: after switching
// "Ask every time" (which deletes the saved one) back to the keychain, or for a connection
// saved without a password, the form claims a password that does not exist, and Test
// connection then fails with "password missing".
// Steps: connection with save_password = true and no keychain entry; Edit.
// Expected: no "(saved)" placeholder when nothing is saved. Actual: "•••••• (saved)".
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx (placeholder from
// `existing && conn.save_password`, not from the keychain).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { editConnection, treeRow } from "../conn-helpers.mjs";

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "7a0e0c3e-5b1d-4c1e-9a55-0000000c0e03"
name = "QA nothing saved"
driver = "postgres"
host = "db.invalid"
port = 5432
database = "app"
user = "me"
save_password = true
`,
  );
}

export default async function (page) {
  await page.waitFor(`${treeRow("QA nothing saved")} !== undefined`);
  await editConnection(page, "QA nothing saved");
  const placeholder = await page.eval(`document.querySelector('.connection-dialog input[type=password]').placeholder`);
  assert.doesNotMatch(placeholder, /saved/, `placeholder: ${placeholder}`);
}
