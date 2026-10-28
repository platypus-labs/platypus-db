// URL-03 (low). In MySQL's URL mode `ssl-mode=` is read but `sslMode=` (Connector/J 8 and
// MySQL Shell write it so) is "Not used here": `mysql://u@h/db?sslMode=DISABLED` keeps the
// default TLS mode, and `sslMode=REQUIRED` silently does not require TLS.
// Steps: New connection > MySQL > Connect by URL; type
// mysql://app@db.example:3306/shop?sslMode=REQUIRED; leave the field.
// Expected: TLS "require" (the printed URL has sslmode=require), no note.
// Actual: note "Not used here: sslMode", printed URL without sslmode.
// Suspected: crates/platypus-core/src/config.rs:410 (`"sslmode" | "ssl-mode"` only).
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { cancel, enterUrl, leaveUrl, newUrlForm, notes, url, urlError } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newUrlForm(page, "MySQL");
  await enterUrl(page, "mysql://app@db.example:3306/shop?sslMode=REQUIRED");
  assert.equal(await page.eval(urlError), "");
  const left = await page.eval(notes);
  await leaveUrl(page);
  const printed = await page.eval(`${url}.value`);
  await cancel(page);
  assert.match(printed, /sslmode=require/, `sslMode=REQUIRED dropped: ${printed} (note: ${left})`);
}
