// SCH-10 (low). After Compare with… finds "nothing to change", the notice stays when another
// target schema is picked, so it reads as the answer for the new schema before any comparison
// (a user may close the dialog believing qa3s_other is up to date). Also guards Escape, which
// closes Compare with… and Check schema… right after they open.
// Steps: qa3s_src and qa3s_dst identical, qa3s_other different. Navigator > qa3s_src > Compare
// with… > Escape; Check schema… > Escape; Compare with… qa3s_dst > Compare ("nothing to
// change"), then pick qa3s_other in Schema.
// Expected: the notice disappears when the target schema changes. Actual: it stays.
// Suspected: apps/desktop/src/schema/CompareDialog.tsx:86 (Schema onChange sets the schema
// only; setSame(false) runs only when the connection changes).
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, compareDialog, lintDialog, needsEngines, openPath, rowMenu, saveEngines } from "../schema-compare-helpers.mjs";

export const skip = needsEngines("pg");
const OTHER = "qa3s_other";

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.t (id int PRIMARY KEY, name text);`);
  ENGINES.pg.seed(dir, DST, `CREATE TABLE S.t (id int PRIMARY KEY, name text);`);
  ENGINES.pg.seed(dir, OTHER, `CREATE TABLE S.t (id int PRIMARY KEY);`);
  saveEngines(dir, ["pg"]);
}

const pick = (page, value) =>
  page.eval(`(() => {
    const s = ${compareDialog}.querySelectorAll('select')[1];
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, ${JSON.stringify(value)});
    s.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);

export default async function (page) {
  const problems = [];
  try {
    await openPath(page, ["QA3 PG", "platypus", SRC]);
    const chain = ["QA3 PG", "platypus"];

    await rowMenu(page, chain, SRC, "Compare with");
    await page.waitFor(`${compareDialog} !== null`);
    await page.key("Escape", "Escape", 27);
    await page.sleep(500);
    if (await page.eval(`${compareDialog} !== null`)) {
      problems.push("Escape does not close Compare with…");
      await page.eval(`${buttonIn(compareDialog, "Cancel")}.click()`);
    }

    await rowMenu(page, chain, SRC, "Check schema");
    await page.waitFor(`${lintDialog} !== null && !${lintDialog}.textContent.includes('Reading the tables')`, 20_000);
    await page.key("Escape", "Escape", 27);
    await page.sleep(500);
    if (await page.eval(`${lintDialog} !== null`)) {
      problems.push("Escape does not close Check schema…");
      await page.eval(`${buttonIn(lintDialog, "Close")}.click()`);
    }

    await rowMenu(page, chain, SRC, "Compare with");
    await page.waitFor(`[...(${compareDialog}?.querySelectorAll('select')[1]?.options ?? [])].some(o => o.value === ${JSON.stringify(OTHER)})`, 20_000);
    await pick(page, DST);
    await page.eval(`${buttonIn(compareDialog, "Compare")}.click()`);
    await page.waitFor(`${compareDialog}.querySelector('.notice') !== null`, 20_000);
    assert.match(await page.eval(`${compareDialog}.querySelector('.notice').textContent`), /nothing to change/);
    await pick(page, OTHER);
    await page.sleep(300);
    const stale = await page.eval(`${compareDialog}.querySelector('.notice')?.textContent ?? null`);
    if (stale !== null) problems.push(`after picking ${OTHER} the dialog still says: ${stale}`);
    await page.eval(`${buttonIn(compareDialog, "Cancel")}.click()`);
  } finally {
    for (const s of [SRC, DST, OTHER]) ENGINES.pg.drop(s);
  }
  assert.deepEqual(problems, []);
}
