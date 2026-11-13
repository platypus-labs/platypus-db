// CONN-10 (medium, security). A login typed (or pasted as plain text) into the Host field,
// `root:s3cret@db.example.com:3306`, is saved as the host: the password lands in clear in
// connections.toml, and Copy as URL hands it out as `mysql://[root:s3cret@db.example.com:3306]:3306/`
// (a broken URL that carries the password). A pasted URL with such a host is refused
// ("the host is not valid: it holds `@`…"), but Save does not check the host.
// Steps: New connection, MySQL; Name "QA leak"; Host `root:s3cret@db.example.com:3306`;
// Save; Copy as URL.
// Expected: Save refuses the host (or splits it); no password in connections.toml or in the
// URL. Actual: saved, and the URL contains it.
// Suspected: crates/platypus-store/src/connections.rs `check_connection` (no host check) and
// crates/platypus-core/src/config/to_url.rs `authority` (host written as is).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, fill, formField, menu, menuItem, newConnection, seedSqlite, treeRow } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, { dir }) {
  await newConnection(page, "MySQL");
  await page.eval(fill(formField("Name"), "QA leak"));
  await page.eval(fill(formField("Host"), "root:s3cret@db.example.com:3306"));
  await page.eval(fill(formField("Password from"), "ask"));
  await page.eval(`${button(dialog, "Save")}.click()`);
  const refused = `document.querySelector('.connection-dialog .notice.error') !== null`;
  await page.waitFor(`${dialog} === null || ${refused}`);
  if (await page.eval(`${dialog} === null`)) {
    assert.doesNotMatch(readFileSync(join(dir, "connections.toml"), "utf8"), /s3cret/, "the password is saved in connections.toml");
    await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
    await page.eval(`${treeRow("QA leak")}.id = 'qa-leak'`);
    await menu(page, "#qa-leak");
    await menuItem(page, "Copy as URL");
    await page.waitFor(`window.__copied !== undefined`);
    assert.doesNotMatch(await page.eval(`window.__copied`), /s3cret/, "Copy as URL hands out the password");
  }
}
