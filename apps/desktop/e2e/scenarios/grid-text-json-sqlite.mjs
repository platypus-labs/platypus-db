// JSON in a text column on a throwaway SQLite database: the value panel opens it indented;
// on a connection that keeps it as written it opens as text, with the JSON view a pick away.
// Text that is not JSON (a number, a broken object) stays text either way. The option is
// a checkbox of the connection dialog, saved only when turned off.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { treeMenu } from "../qa-grid.mjs";

const FORMAT = "E2E text json";
const PLAIN = "E2E text json plain";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const treeRow = (label, under) =>
  `[...(${under ?? "document"}).querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const panel = `document.querySelector('.value-panel')`;
const mode = `(${panel}?.querySelector('select')?.value ?? '')`;
const shown = `(${panel}?.querySelector('.value-text')?.textContent ?? '')`;

export function setup({ dir }) {
  const path = join(dir, "json.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE docs (id INTEGER PRIMARY KEY, body TEXT);
           INSERT INTO docs VALUES (1, '{"a":1,"b":[true,null]}'), (2, '42'), (3, '{"broken":');`);
  db.close();
  const connection = (id, name, extra) =>
    `[[connections]]\nid = "00000000-0000-4000-8000-0000000b00${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${path}'\nsave_password = false\n${extra}`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1\n\n${connection("11", FORMAT, "")}\n${connection("12", PLAIN, 'text_json = "plain"\n')}`,
  );
}

/** Opens `docs` of a connection and focuses the `body` of row `row`, the value panel open. */
async function openBody(page, connection, row) {
  await page.waitFor(`${treeRow(connection)} !== undefined`);
  const expanded = await page.eval(`${treeRow(connection)}.closest('[role=treeitem]')?.getAttribute('aria-expanded') === 'true'`);
  if (!expanded) await page.eval(`${treeRow(connection)}.querySelector(".twisty").click()`);
  await page.waitFor(`[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === 'docs').length > 0`);
  // The `docs` row under this connection: the first one after its row.
  await page.eval(`(() => {
    const rows = [...document.querySelectorAll('.tree-row')];
    const at = rows.indexOf(${treeRow(connection)});
    rows.slice(at + 1).find(r => r.querySelector('.tree-label')?.textContent === 'docs').id = 'e2e-docs';
  })()`);
  await page.clickSel("#e2e-docs", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-docs').removeAttribute('id')`);
  await page.waitFor(`${status} === '3 rows'`, 15_000);
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("ArrowRight", "ArrowRight", 39);
  if (!(await page.eval(`${panel} !== null`))) await page.key("F7", "F7", 118);
  await page.waitFor(`${panel} !== null && ${panel}.querySelector('.value-name')?.textContent === 'body'`);
}

async function down(page) {
  await page.eval(`${grid}.focus()`);
  await page.key("ArrowDown", "ArrowDown", 40);
}

export default async function (page, { dir }) {
  // Indented by default.
  await openBody(page, FORMAT, 0);
  await page.waitFor(`${mode} === 'json' && ${shown}.includes('"b": [')`);
  assert.match(await page.eval(shown), /\{\n {2}"a": 1,/);
  // A number and a broken object are text.
  await down(page);
  await page.waitFor(`${mode} === 'text' && ${shown} === '42'`);
  assert.ok(!(await page.eval(`[...${panel}.querySelectorAll('option')].some(o => o.value === 'json')`)));
  await down(page);
  await page.waitFor(`${mode} === 'text' && ${shown} === '{"broken":'`);
  await page.key("w", "KeyW", 87, CTRL);

  // Kept as written on the other connection, with the JSON view still there.
  await openBody(page, PLAIN, 0);
  await page.waitFor(`${mode} === 'text' && ${shown} === '{"a":1,"b":[true,null]}'`);
  await page.eval(`(() => {
    const select = ${panel}.querySelector('select');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, 'json');
    select.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
  await page.waitFor(`${mode} === 'json' && ${shown}.includes('"b": [')`);
  await page.key("F7", "F7", 118);
  await page.waitFor(`${panel} === null`);
  await page.key("w", "KeyW", 87, CTRL);

  // The checkbox of the connection dialog: on for the first, off for the other; turning it
  // on again saves nothing.
  const dialog = `document.querySelector('.modal.connection-dialog')`;
  const checkbox = `${dialog}?.querySelector('[data-field=text_json]')`;
  for (const [name, checked] of [
    [FORMAT, true],
    [PLAIN, false],
  ]) {
    await treeMenu(page, name, "Edit…");
    await page.waitFor(`${checkbox} != null`);
    assert.equal(await page.eval(`${checkbox}.checked`), checked, name);
    if (!checked) {
      await page.eval(`${checkbox}.click()`);
      await page.waitFor(`${checkbox}.checked`);
      await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Save').click()`);
      await page.waitFor(`${dialog} === null`);
    } else {
      await page.key("Escape", "Escape", 27);
      await page.waitFor(`${dialog} === null`);
    }
  }
  const saved = readFileSync(join(dir, "connections.toml"), "utf8");
  assert.ok(!saved.includes("text_json"), saved);
}
