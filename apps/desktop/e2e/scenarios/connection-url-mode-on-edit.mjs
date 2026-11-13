// URL-12 (low). The Host or URL choice is not kept: a connection created in URL mode opens
// in Host mode when edited (and its duplicate too); the user has to choose URL again each
// time. The settings themselves are kept: the duplicate prints the same URL.
// Steps: New connection > MySQL > Connect by URL; type a URL; save; Edit.
// Expected: the form opens in URL mode, the way the connection was entered.
// Actual: Host mode.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx:117 (`useState<ConnectBy>
// ("host")`; the choice is not saved with the connection).
import assert from "node:assert/strict";
import { editConnection, fill, formField, menu, menuItem, saveForm, seedSqlite, treeRow } from "../conn-helpers.mjs";
import { byUrl, cancel, enterUrl, leaveUrl, newUrlForm, toUrl, url } from "../connection-url-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

const names = `[...document.querySelectorAll('.tree-row .tree-label')].map(l => l.textContent)`;

export default async function (page) {
  await newUrlForm(page, "MySQL");
  await page.eval(fill(formField("Name"), "QA url mode"));
  await enterUrl(page, "mysql://app@db.example:3307/shop?sslmode=require");
  await leaveUrl(page);
  const printed = await page.eval(`${url}.value`);
  assert.equal(printed, "mysql://app@db.example:3307/shop?sslmode=require");
  await saveForm(page);
  await page.waitFor(`${treeRow("QA url mode")} !== undefined`);

  // Duplicate: same settings.
  const before = await page.eval(names);
  await page.eval(`${treeRow("QA url mode")}.id = 'qa-dup'`);
  await menu(page, "#qa-dup");
  await menuItem(page, "Duplicate");
  await page.waitFor(`${names}.length > ${before.length}`, 10_000);
  const copy = (await page.eval(names)).find((n) => !before.includes(n));
  await editConnection(page, copy);
  const copyMode = await page.eval(`${byUrl}.getAttribute('aria-checked')`);
  await toUrl(page);
  assert.equal(await page.eval(`${url}.value`), printed);
  await cancel(page);

  await editConnection(page, "QA url mode");
  const mode = await page.eval(`${byUrl}.getAttribute('aria-checked')`);
  await cancel(page);
  assert.equal(mode, "true", "a connection entered as a URL opens in Host mode");
  assert.equal(copyMode, "true", "its duplicate opens in Host mode");
}
