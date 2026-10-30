// The welcome page's Recent list also names the scripts opened or saved last: each opens
// its file in an editor on click, a file that is gone is reported and opens nothing, and
// with none remembered (and no history) the section is not drawn at all.
import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const CONN = "00000000-0000-4000-8000-0000000000d1";
const KEY = "platypus.recent-scripts.v1";

export function setup({ dir }) {
  const file = join(dir, "recent.db");
  new DatabaseSync(file).close();
  mkdirSync(join(dir, "scripts"), { recursive: true });
  writeFileSync(join(dir, "scripts", "revenue.sql"), "SELECT 'revenue'");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 2\n\n[[connections]]\nid = "${CONN}"\nname = "Recent db"\ndriver = "sqlite"\ndatabase = '${file}'\nsave_password = false\n`,
  );
}

const rows = `[...document.querySelectorAll('.welcome-recent [data-script]')]`;
const tabCount = `document.querySelectorAll('.tab').length`;

/** Opens a tab and closes it, so the welcome page is drawn again and reads the list. */
async function remount(page) {
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabCount} === 1`);
  await page.eval(`document.querySelector('.tab [aria-label="Close tab"]').click()`);
  await page.waitFor(`${tabCount} === 0 && document.querySelector('.welcome') !== null`);
}

export default async function (page, { dir }) {
  await page.waitFor(`document.querySelector('.welcome') !== null`);
  // Nothing remembered: no list, no empty box.
  assert.equal(await page.eval(`document.querySelector('.welcome-recent')`), null);

  const real = join(dir, "scripts", "revenue.sql");
  const gone = join(dir, "scripts", "gone.sql");
  const stored = [
    { path: real, connectionId: CONN, at: Date.now() - 2 * 3_600_000 },
    { path: gone, connectionId: CONN, at: Date.now() - 3 * 86_400_000 },
  ];
  await page.eval(`localStorage.setItem(${JSON.stringify(KEY)}, ${JSON.stringify(JSON.stringify(stored))})`);
  await remount(page);
  await page.waitFor(`${rows}.length === 2`);
  assert.deepEqual(await page.eval(`${rows}.map(r => r.querySelector('.welcome-name').textContent)`), ["revenue.sql", "gone.sql"]);
  assert.equal(await page.eval(`${rows}[0].title`), real);

  // A missing file opens nothing.
  await page.eval(`${rows}[1].click()`);
  await page.sleep(500);
  assert.equal(await page.eval(tabCount), 0);

  // A click opens the script in an editor, and it stays first in the list afterwards.
  await page.eval(`${rows}[0].click()`);
  await page.waitFor(`${tabCount} === 1`);
  await page.waitFor(`document.querySelector('.cm-content')?.textContent === "SELECT 'revenue'"`);
  await page.eval(`document.querySelector('.tab [aria-label="Close tab"]').click()`);
  await page.waitFor(`${tabCount} === 0 && ${rows}.length === 2`);
  assert.equal(await page.eval(`${rows}[0].dataset.script`), real);
}
