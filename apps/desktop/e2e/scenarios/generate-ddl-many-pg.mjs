// "Generate DDL" on many tables of PostgreSQL: the tab opens at once and fills a block of
// objects at a time, saying how far it is ("DDL: 30 of 300 objects") with Stop; the DDL comes
// in the order of the selection, a table dropped meanwhile becomes a comment saying why, and
// Stop leaves what was read with a note of how far it went.
import assert from "node:assert/strict";
import { ENGINES, SRC, needsEngines, openPath, psql, queryText, saveEngines, visibleQuery } from "../qa3-schema.mjs";
import { SHIFT } from "../cdp.mjs";
import { revealItem } from "../menu-sub.mjs";

export const skip = needsEngines("pg");

const TABLES = 300;
const name = (i) => `t${String(i).padStart(4, "0")}`;

export function setup({ dir }) {
  const tables = Array.from(
    { length: TABLES },
    (_, i) => `CREATE TABLE S.${name(i + 1)} (id int PRIMARY KEY, code text UNIQUE, qty int CHECK (qty >= 0));
CREATE INDEX ${name(i + 1)}_qty ON S.${name(i + 1)} (qty);
COMMENT ON TABLE S.${name(i + 1)} IS 'table ${i + 1}';`,
  );
  ENGINES.pg.seed(dir, SRC, tables.join("\n"));
  saveEngines(dir, ["pg"]);
}

const row = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const progress = `${visibleQuery}?.querySelector('.ddl-progress')`;

/** Clicks the tree row `label`, scrolling the tree until it is drawn. */
async function clickRow(page, label, options = {}) {
  const scroll = (by) => page.eval(`(() => { let e = document.querySelector('.tree-row'); while (e && e.scrollHeight <= e.clientHeight) e = e.parentElement; if (e) e.scrollTop ${by === null ? "= 0" : `+= ${by}`}; })()`);
  // The tree draws only the rows in view: from the top, scroll its scrolling box down.
  if (!(await page.eval(`${row(label)} !== undefined`))) await scroll(null);
  for (let i = 0; i < 100 && !(await page.eval(`${row(label)} !== undefined`)); i++) {
    await scroll(400);
    await page.sleep(100);
  }
  await page.eval(`${row(label)}.scrollIntoView({ block: 'center' }); ${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", options);
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
}

/** Selects every table, opens the menu on the last one and runs Generate DDL. */
async function generateAll(page) {
  await clickRow(page, name(1));
  await clickRow(page, name(TABLES), { modifiers: SHIFT });
  // The tree draws only the rows in view, so the 300 are counted by the tab title below: here
  // the last row is selected and so is every table row drawn.
  await page.waitFor(
    `${row(name(TABLES))}?.classList.contains('selected') && [...document.querySelectorAll('.tree-row')].filter(r => /^t\d{4}$/.test(r.querySelector('.tree-label')?.textContent ?? '')).every(r => r.classList.contains('selected'))`,
    10_000,
  );
  // The menu of the selection: a right click the tree redraws under is repeated.
  const deadline = Date.now() + 30_000;
  for (;;) {
    await clickRow(page, name(TABLES), { button: "right" });
    try {
      await page.waitFor(`document.querySelector('.context-menu') !== null`, 2_000);
      break;
    } catch (e) {
      if (Date.now() > deadline) throw e;
    }
  }
  await revealItem(page, "Generate DDL");
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Generate DDL')).click()`);
}

export default async function (page) {
  await openPath(page, ["QA3 PG", "platypus", SRC]);
  await page.eval(`${row(SRC)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row(name(1))} !== undefined`, 30_000);

  // A table dropped outside the app after the navigator listed it.
  psql(`DROP TABLE ${SRC}.${name(150)}`);
  await generateAll(page);
  // The tab is there before the DDL: it opens at once, empty, and says how far it is.
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('DDL of ${TABLES} objects')`, 10_000);
  await page.waitFor(`/^DDL: \\d+ of ${TABLES} objects/.test(${progress}?.textContent ?? '')`, 10_000);
  await page.waitFor(`${progress} == null`, 120_000);
  const ddl = await page.eval(queryText);
  const creates = [...ddl.matchAll(/CREATE TABLE \S+?\.?(t\d{4})/g)].map((m) => m[1]);
  assert.equal(creates.length, TABLES - 1, "every table but the dropped one");
  assert.deepEqual(creates, Array.from({ length: TABLES }, (_, i) => name(i + 1)).filter((n) => n !== name(150)), "in the order of the selection");
  assert.match(ddl, new RegExp(`-- ${SRC}\\.${name(150)}: .*not found`), "the dropped table says why");
  assert.match(ddl, /COMMENT ON TABLE .*t0001.* IS 'table 1'/);
  assert.match(ddl, /CREATE INDEX t0300_qty/);
  assert.equal(await page.eval(`${visibleQuery}.querySelector('.notice.error')`), null);

  // Stop: clicked while the DDL is read, in the same turn the progress shows, so it is still
  // running. What was read stays, with a note of how far it went; nothing more comes after.
  await generateAll(page);
  await page.waitFor(
    `(() => { const p = ${progress}; const b = p?.querySelector('button'); if (!b || !/^DDL: [1-9]/.test(p.textContent)) return false; b.click(); return true; })()`,
    30_000,
  );
  await page.waitFor(`${progress} == null`, 30_000);
  // A block asked for before Stop is still added, and the editor takes it a moment after the
  // progress goes: read the text once it has not changed for a while.
  let stopped = await page.eval(queryText);
  for (let same = 0; same < 3; ) {
    await page.sleep(300);
    const text = await page.eval(queryText);
    same = text === stopped ? same + 1 : 0;
    stopped = text;
  }
  const note = /-- Stopped: the DDL of (\d+) of (\d+) objects was read/.exec(stopped);
  assert.ok(note, `a note says where it stopped: ${stopped.slice(-300)}`);
  assert.equal(Number(note[2]), TABLES);
  assert.ok(Number(note[1]) < TABLES);
  const read = [...stopped.matchAll(/CREATE TABLE /g)].length;
  assert.ok(read > 0 && read <= Number(note[1]), `${read} tables read, ${note[1]} said`);
  await page.sleep(1500);
  assert.equal(await page.eval(queryText), stopped, "nothing comes after Stop");
}
