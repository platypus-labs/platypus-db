// Regression guard (passes): Compare with… when the target connection picked does not answer:
// the connection error is shown (as a notice of the window, not of the dialog), Schema stays empty, Compare is
// disabled and Close is not; picking a working connection again lists its schemas, with no
// error left in the dialog, and compares ("nothing to change"); leaving it for the one down
// again, its schema does not stay and Compare is disabled.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, compareDialog, needsEngines, openPath, rowMenu, saveEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, `CREATE TABLE S.t (id int PRIMARY KEY);`);
  ENGINES.pg.seed(dir, DST, `CREATE TABLE S.t (id int PRIMARY KEY);`);
  saveEngines(dir, ["pg"], [
    {
      id: "00000000-0000-4000-8000-0000000c3103",
      name: "QA3 PG down",
      body: `driver = "postgres"\nhost = "127.0.0.1"\nport = 55999\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nshow_all_databases = true\npassword_command = "echo platypus"`,
    },
  ]);
}

const pickConnection = (page, name) =>
  page.eval(`(() => {
    const s = ${compareDialog}.querySelectorAll('select')[0];
    const o = [...s.options].find(o => o.textContent === ${JSON.stringify(name)});
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, o.value);
    s.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);

export default async function (page) {
  try {
    const chain = ["QA3 PG", "platypus"];
    await openPath(page, [...chain, SRC]);
    await rowMenu(page, chain, SRC, "Compare with");
    await page.waitFor(`${compareDialog} !== null`);
    await pickConnection(page, "QA3 PG down");
    await page.waitFor(`[...document.querySelectorAll('.notice, .toast')].some(n => n.textContent.includes('QA3 PG down'))`, 30_000);
    assert.equal(await page.eval(`${compareDialog}.querySelectorAll('select')[1].options.length`), 0);
    assert.equal(await page.eval(`${buttonIn(compareDialog, "Compare")}.disabled`), true, "Compare disabled without a schema");
    assert.equal(await page.eval(`${buttonIn(compareDialog, "Cancel")}.disabled`), false);
    await pickConnection(page, "QA3 PG");
    await page.waitFor(`[...${compareDialog}.querySelectorAll('select')[1].options].some(o => o.value === ${JSON.stringify(DST)})`, 20_000);
    const left = await page.eval(`${compareDialog}.querySelector('.notice.error')?.textContent ?? null`);
    assert.equal(left, null, "the error of the other connection stays");
    await page.eval(`(() => {
      const s = ${compareDialog}.querySelectorAll('select')[1];
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, ${JSON.stringify(DST)});
      s.dispatchEvent(new Event('change', { bubbles: true }));
    })()`);
    await page.eval(`${buttonIn(compareDialog, "Compare")}.click()`);
    await page.waitFor(`${compareDialog}.querySelector('.notice') !== null`, 20_000);
    assert.match(await page.eval(`${compareDialog}.querySelector('.notice').textContent`), /nothing to change/);
    // Leaving a target whose schema was chosen: the schema does not stay, Compare waits.
    // The first error is still on screen: dismiss it, so the next one is a new attempt failing.
    await page.eval(`document.querySelector('.toast')?.click()`);
    await page.waitFor(`document.querySelector('.toast') === null`);
    await pickConnection(page, "QA3 PG down");
    await page.waitFor(`[...document.querySelectorAll('.toast')].some(n => n.textContent.includes('QA3 PG down'))`, 30_000);
    assert.equal(await page.eval(`${compareDialog}.querySelectorAll('select')[1].value`), "", "the other connection's schema stays");
    assert.equal(await page.eval(`${buttonIn(compareDialog, "Compare")}.disabled`), true, "Compare enabled on a schema never chosen");
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
}
