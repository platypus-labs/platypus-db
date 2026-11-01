// A copy action confirms with a toast: "Copy name" in the navigator, Ctrl+C in the grid, and a
// failed clipboard write says so. The toast is a polite live region and goes by itself.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "Copy toast db";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const toasts = `[...document.querySelectorAll('.toaster .toast-item')].map(t => t.textContent)`;

export function setup({ dir }) {
  const path = join(dir, "copy-toast.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO items VALUES (1, 'apple'), (2, 'banana');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000031"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

/** Waits for the previous toast to go, so a text already there is not taken for the new one. */
const toastsGone = (page) => page.waitFor(`${toasts}.length === 0`, 10_000);

export default async function (page) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("items")} !== undefined`, 15_000);

  // The toaster is there from the start, a polite live region, and empty.
  assert.equal(await page.eval(`document.querySelector('.toaster')?.getAttribute('role')`), "status");
  assert.equal(await page.eval(`document.querySelector('.toaster')?.getAttribute('aria-live')`), "polite");
  assert.deepEqual(await page.eval(toasts), []);

  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);

  // Copy name in the navigator.
  await page.eval(`${treeRow("items")}.id = 'e2e-table'`);
  const r = await page.rect("#e2e-table");
  await page.click(r.x + 40, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === 'Copy name').click()`);
  await page.waitFor(`${toasts}.includes('Copied')`);
  assert.ok((await page.eval(`window.__copied`)).includes("items"));
  // It goes by itself.
  await toastsGone(page);

  // Ctrl+C in the grid.
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '2 rows'`, 15_000);
  await page.eval(`${grid}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.eval(`window.__copied = undefined`);
  await page.key("c", "KeyC", 67, CTRL);
  await page.waitFor(`${toasts}.includes('Copied')`);
  assert.ok((await page.eval(`window.__copied`)).includes("banana"));
  await toastsGone(page);

  // A clipboard that refuses says so, and nothing says "Copied".
  await page.eval(`navigator.clipboard.writeText = async () => { throw new Error('denied'); }`);
  await page.key("c", "KeyC", 67, CTRL);
  await page.waitFor(`${toasts}.includes('Could not copy to the clipboard.')`);
  assert.ok(!(await page.eval(toasts)).includes("Copied"));
  // A click dismisses it.
  await page.eval(`document.querySelector('.toaster .toast-item').click()`);
  await page.waitFor(`${toasts}.length === 0`);
}
