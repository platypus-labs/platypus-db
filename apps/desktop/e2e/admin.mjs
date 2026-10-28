// Helpers of the Server activity scenarios (group J): the admin tab, its session rows, and
// sessions of the dev databases run in the background from a shell in their container.
import { spawn } from "node:child_process";
import { DBS, assert, expand, treeMenu } from "./qa-grid.mjs";

export const adminTab = `[...document.querySelectorAll('.admin-tab')].find(t => t.style.display !== 'none')`;

/** The session row whose statement contains `text`. */
export const rowWith = (text) =>
  `[...(${adminTab}?.querySelectorAll('tr.admin-session') ?? [])].find(r => r.querySelector('.admin-query')?.textContent.includes(${JSON.stringify(text)}))`;

export const adminButton = (label) =>
  `[...${adminTab}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)})`;

/** Connects `name` and opens Server activity on it. */
export async function openAdmin(page, name, db) {
  await expand(page, name, db);
  await treeMenu(page, name, "Server activity");
  await page.waitFor(`${adminTab} !== undefined`, 15_000);
}

/** A session of `db` running `sql` in the background; `exited` resolves to its output. */
export function background(db, sql) {
  const child = spawn("docker", DBS[db].shell, { stdio: ["pipe", "pipe", "pipe"] });
  let output = "";
  child.stdout.on("data", (d) => (output += d));
  child.stderr.on("data", (d) => (output += d));
  const exited = new Promise((resolve) => child.on("exit", () => resolve(output)));
  child.stdin.end(sql);
  return { child, exited };
}

/** Answers the open confirmation with its red button. */
export async function confirmDanger(page) {
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
}

/** The id of the session row whose statement contains `text`. */
export const idOf = (page, text) => page.eval(`${rowWith(text)}.children[0].textContent.trim()`);

/** Waits for the session running `text` to be listed as waiting for `blocker`. */
export async function waitBlocked(page, text, blocker) {
  await page.waitFor(`${rowWith(text)}?.classList.contains('blocked') ?? false`, 30_000);
  assert.equal(await page.eval(`${rowWith(text)}.children[8].textContent`), blocker);
}

/** Opens the Locks view and checks the chain from `holder` to `waiter`. */
export async function checkChain(page, holder, waiter) {
  await page.eval(`[...${adminTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Locks')).click()`);
  await page.waitFor(`${adminTab}.querySelector('.admin-chains') !== null`, 15_000);
  assert.equal(await page.eval(`${adminTab}.querySelector('.admin-chains > li > button .admin-blocker').textContent`), holder);
  assert.equal(await page.eval(`${adminTab}.querySelector('.admin-chains > li > ul .admin-waiter').textContent`), waiter);
  await page.waitFor(
    `[...${adminTab}.querySelectorAll('table.admin-table tbody tr.blocked')].some(r => r.children[0].textContent === ${JSON.stringify(waiter)})`,
    15_000,
  );
  await page.eval(`[...${adminTab}.querySelectorAll('.object-sections button')].find(b => b.textContent.startsWith('Sessions')).click()`);
  await page.waitFor(`${adminTab}.querySelector('input[type=search]') !== null`);
}
