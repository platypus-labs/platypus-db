// Waiting and empty states (group T): an empty result is an empty state, not a spinner. Check
// schema… on a clean schema says "No problems found." without aria-busy; Snapshots… with none
// taken says how one is taken, without aria-busy; the object page's Refresh shows the busy
// indicator (button and body) while it reloads and drops it when done. Not covered here: the
// Test connection / Sync now spinners, which need a slow server (no deterministic SQLite case).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { palette } from "../plans.mjs";
import { lintDialog, openPath, rowMenu } from "../schema-compare-helpers.mjs";

const NAME = "Ux db";
const modalBusy = (sel) => `document.querySelector('${sel} [aria-busy]') !== null`;
const objectTab = `[...document.querySelectorAll('.object-tab')].find(t => t.style.display !== 'none')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ux.db"));
  db.exec(`CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000a7b01"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "ux.db")}'
save_password = false
`,
  );
}

const closeModal = (page, sel) =>
  page
    .eval(`[...document.querySelectorAll('${sel} button')].find(b => b.textContent === 'Close').click()`)
    .then(() => page.waitFor(`document.querySelector('${sel}') === null`));

export default async function (page) {
  await openPath(page, [NAME, "main"]);

  // Check schema: nothing wrong is an empty state with its text, never a loading one.
  await rowMenu(page, [NAME], "main", "Check schema");
  await page.waitFor(`${lintDialog} !== null && !${lintDialog}.textContent.includes('Reading the tables')`, 15_000);
  assert.ok((await page.eval(`${lintDialog}.textContent`)).includes("No problems found."));
  assert.equal(await page.eval(modalBusy(".modal.schema-lint")), false, "the empty lint result is not busy");
  assert.equal(await page.eval(`${lintDialog}.querySelector('.state.empty') !== null`), true);
  await closeModal(page, ".modal.schema-lint");

  // Snapshots: none taken yet, the empty state says how one is.
  await palette(page, "Snapshots before changes");
  await page.waitFor(`document.querySelector('.modal.snapshots .snapshots-empty') !== null`, 10_000);
  assert.equal(await page.eval(modalBusy(".modal.snapshots")), false, "the empty snapshot list is not busy");
  const text = await page.eval(`document.querySelector('.modal.snapshots .snapshots-empty').textContent`);
  assert.ok(text.includes("No snapshot yet.") && text.includes("UPDATE or DELETE"), text);
  await closeModal(page, ".modal.snapshots");

  // Object page Refresh: the busy indicator shows while it reloads, and goes when done.
  await openPath(page, [NAME, "main", "users"]);
  await rowMenu(page, [NAME, "main"], "users", "Open details");
  await page.waitFor(`${objectTab} !== undefined && ${objectTab}.querySelector('.object-body table') !== null`, 15_000);
  assert.equal(await page.eval(`${objectTab}.querySelector('.object-body[aria-busy]') === null`), true, "not busy once loaded");
  // A MutationObserver catches the busy state even if the reload is quick.
  await page.eval(`(() => {
    window.__sawBusy = false;
    const body = ${objectTab}.querySelector('.object-body');
    new MutationObserver(() => { if (body.hasAttribute('aria-busy')) window.__sawBusy = true; }).observe(body, { attributes: true, attributeFilter: ['aria-busy'] });
    ${objectTab}.querySelector('[data-action="refresh-object"]').click();
  })()`);
  await page.waitFor(`window.__sawBusy === true`, 10_000);
  await page.waitFor(`${objectTab}.querySelector('.object-body[aria-busy]') === null && ${objectTab}.querySelector('[data-action="refresh-object"][aria-busy]') === null`, 15_000);
  assert.equal(await page.eval(`${objectTab}.querySelector('.object-body').classList.contains('is-reloading')`), false);
}
