// Schema tools on a PostgreSQL schema of 300 tables with foreign keys (each table references
// one of the ten before it, and has an index on the key): the ER diagram of all of them, Check
// schema… and Compare with… an identical copy, each timed. A demanding user expects each within
// a few seconds; the times are printed.
// Steps: qa3s_src and qa3s_dst with the same 300 tables; ER diagram (picker: Select shown,
// Show); Check schema…; Compare with… qa3s_dst.
// Expected: diagram < 15 s, check < 15 s, comparison < 20 s ("nothing to change").
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, compareUi, lintDialog, needsEngines, openPath, psql, rowMenu, saveEngines, visibleErd } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");
const N = 300;
const picker = `document.querySelector('.modal.erd-tables')`;

const tables = (schema) => `DO $$ BEGIN
  FOR i IN 1..${N} LOOP
    IF i = 1 THEN
      EXECUTE format('CREATE TABLE ${schema}.t_%s (id int PRIMARY KEY, label text NOT NULL, amount numeric(10,2) DEFAULT 0)', i);
    ELSE
      EXECUTE format('CREATE TABLE ${schema}.t_%s (id int PRIMARY KEY, parent_id int REFERENCES ${schema}.t_%s(id), label text NOT NULL, amount numeric(10,2) DEFAULT 0)', i, greatest(1, i - 1 - (i % 10)));
      EXECUTE format('CREATE INDEX ON ${schema}.t_%s (parent_id)', i);
    END IF;
  END LOOP;
END $$;`;

export function setup({ dir }) {
  ENGINES.pg.seed(dir, SRC, "");
  ENGINES.pg.seed(dir, DST, "");
  psql(tables(SRC));
  psql(tables(DST));
  saveEngines(dir, ["pg"]);
}

async function timed(label, action) {
  const started = Date.now();
  await action();
  const took = Date.now() - started;
  console.log(`  ${label}: ${took} ms`);
  return took;
}

export default async function (page) {
  const chain = ["QA3 PG", "platypus"];
  const problems = [];
  try {
    await openPath(page, [...chain, SRC]);
    await rowMenu(page, chain, SRC, "ER diagram");
    await page.waitFor(`${picker} !== null`, 30_000);
    await page.eval(`${buttonIn(picker, "Select shown")}.click()`);
    const erd = await timed("ER diagram of 300 tables", async () => {
      await page.eval(`${buttonIn(picker, "Show")}.click()`);
      await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === ${N}`, 120_000);
    });
    assert.equal(await page.eval(`${visibleErd}.querySelectorAll('path[marker-start]').length`), N - 1);
    if (erd > 15_000) problems.push(`ER diagram took ${erd} ms`);

    const lint = await timed("Check schema of 300 tables", async () => {
      await rowMenu(page, chain, SRC, "Check schema");
      await page.waitFor(`${lintDialog} !== null && !${lintDialog}.textContent.includes('Reading the tables')`, 120_000);
    });
    assert.match(await page.eval(`${lintDialog}.textContent`), /No problems found/);
    await page.eval(`${buttonIn(lintDialog, "Close")}.click()`);
    if (lint > 15_000) problems.push(`Check schema took ${lint} ms`);

    let result;
    const compare = await timed("Compare 300 tables", async () => {
      result = await compareUi(page, chain, SRC, "QA3 PG", DST, 180_000);
    });
    assert.equal(result.same, true, JSON.stringify(result).slice(0, 500));
    if (compare > 20_000) problems.push(`Compare with… took ${compare} ms`);
  } finally {
    ENGINES.pg.drop(SRC);
    ENGINES.pg.drop(DST);
  }
  assert.deepEqual(problems, []);
}
