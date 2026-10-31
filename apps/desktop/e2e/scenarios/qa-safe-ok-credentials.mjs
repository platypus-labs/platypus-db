// Regression guard (passes): a password saved from the connection dialog goes to the OS
// keychain (Windows Credential Manager, target "<id>.platypusdb"), never into
// connections.toml or any other file of the profile; a failed login does not show it, nor
// does the Activity log; "Copy as URL" leaves it out; deleting the connection removes it from
// the keychain. The password is a wrong one, so the test never stores a working credential.
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { spawnSync } from "../proc.mjs";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { needs } from "../qa-safe-db.mjs";

const ID = "00000000-0000-4000-8000-000000000541";
const NAME = "QA creds PG";
const SECRET = "QaSafeSecret-7319";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, text) =>
  `[...document.querySelectorAll(${JSON.stringify(`${scope} button`)})].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const setLabeled = (scope, label, value) => `(() => {
  const el = [...document.querySelectorAll(${JSON.stringify(`${scope} label.field`)})]
    .find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)}).querySelector('input');
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('input', { bubbles: true }));
})()`;
const inKeychain = () => spawnSync("cmdkey", ["/list"], { encoding: "utf8" }).stdout.includes(`${ID}.platypusdb`);

export const skip = needs("postgres");

export function setup({ dir }) {
  // A leftover of an earlier failed run.
  spawnSync("cmdkey", [`/delete:${ID}.platypusdb`], { stdio: "ignore" });
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "${ID}"
name = "${NAME}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = true
`,
  );
}

/** Files of the profile (not WebView2's) that contain the secret. */
function filesWithSecret(dir) {
  const hits = [];
  const walk = (d) => {
    for (const f of readdirSync(d)) {
      const p = join(d, f);
      if (f === "webview") continue;
      if (statSync(p).isDirectory()) walk(p);
      else if (readFileSync(p).includes(SECRET)) hits.push(p);
    }
  };
  walk(dir);
  return hits;
}

async function menu(page, item) {
  await page.eval(`${row(NAME)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`);
}

export default async function (page, { dir }) {
  try {
    await page.waitFor(`${row(NAME)} !== undefined`);
    assert.equal(inKeychain(), false);
    await page.eval(`[...${row(NAME)}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
    await page.waitFor(`document.querySelector('.modal form, form.modal') !== null`);
    await page.eval(setLabeled(".modal", "Password", SECRET));
    await page.eval(`${button(".modal .actions", "Save")}.click()`);
    await page.waitFor(`document.querySelector('.modal') === null`);
    assert.equal(inKeychain(), true);
    assert.ok(!readFileSync(join(dir, "connections.toml"), "utf8").includes(SECRET));

    await page.eval(`${row(NAME)}.querySelector('.twisty').click()`);
    await page.waitFor(`document.body.innerText.includes('password authentication failed')`, 20_000);
    await page.eval(`document.querySelector('.activity-button')?.click()`);
    await page.waitFor(`document.querySelector('.activity-list') !== null`);
    assert.ok(!(await page.eval(`document.body.innerText`)).includes(SECRET));
    await page.key("Escape", "Escape", 27);

    await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
    await menu(page, "Copy as URL");
    await page.waitFor(`window.__copied !== undefined`);
    assert.equal(await page.eval(`window.__copied`), "postgres://platypus@localhost:55432/platypus?sslmode=disable");
    assert.deepEqual(filesWithSecret(dir), []);

    await menu(page, "Delete");
    await page.waitFor(`document.querySelector('.modal')?.textContent.includes(${JSON.stringify(NAME)})`);
    await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Delete').click()`);
    await page.waitFor(`${row(NAME)} === undefined`);
    assert.equal(inKeychain(), false, "the password stayed in the keychain after the connection was deleted");
  } finally {
    spawnSync("cmdkey", [`/delete:${ID}.platypusdb`], { stdio: "ignore" });
  }
}
